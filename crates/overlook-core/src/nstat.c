// Per-process byte counts from the private NetworkStatistics framework (nettop's backend).
// Loaded at runtime so a future macOS that drops or changes it makes per-app report an error, not crash.
// All state is owned by one serial queue `q`.
#include <CoreFoundation/CoreFoundation.h>
#include <dispatch/dispatch.h>
#include <dlfcn.h>
#include <errno.h>
#include <libproc.h>
#include <signal.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

struct ov_proc { int32_t pid; uint64_t rx, tx; char path[1024]; };

typedef void *M, *S;
static M (*create)(CFAllocatorRef, dispatch_queue_t, void (^)(S, void *));
static void (*add_tcp)(M), (*add_udp)(M);
static void (*set_desc)(S, void (^)(CFDictionaryRef)), (*set_counts)(S, void (^)(CFDictionaryRef));
static void (*set_removed)(S, void (^)(void));
static void (*query_desc)(M, void (^)(void)), (*query_counts)(M, void (^)(void));
static CFStringRef k_pid, k_rx_wifi, k_tx_wifi, k_rx_wired, k_tx_wired;

struct src { struct src *next, *prev; int32_t pid; uint64_t rx, tx; };
struct rec { int32_t pid; uint64_t rx, tx, live_rx, live_tx; int live; char *path; };

static dispatch_queue_t q;
static M mgr;
static struct src *srcs;
static struct rec *recs;
static size_t nrecs, caprecs;

// ponytail: linear scan over pids with sockets (a few hundred); hash it if that grows.
static struct rec *rec_for(int32_t pid) {
    for (size_t i = 0; i < nrecs; i++) if (recs[i].pid == pid) return &recs[i];
    // Snapshots prune dead pids every sample; this only bites once per-app is off but the manager still runs.
    if (nrecs >= 4096)
        for (size_t i = 0; i < nrecs;) {
            if (kill(recs[i].pid, 0) == -1 && errno == ESRCH) free(recs[i].path), recs[i] = recs[--nrecs];
            else i++;
        }
    if (nrecs == caprecs) {
        size_t c = caprecs ? caprecs * 2 : 64;
        struct rec *r = realloc(recs, c * sizeof *r);
        if (!r) return NULL;
        recs = r, caprecs = c;
    }
    char *path = strdup("");
    if (!path) return NULL;
    recs[nrecs] = (struct rec){ .pid = pid, .path = path };
    return &recs[nrecs++];
}

static uint64_t num(CFDictionaryRef d, CFStringRef k) {
    int64_t v = 0;
    CFTypeRef n = d ? CFDictionaryGetValue(d, k) : NULL;
    if (n && CFGetTypeID(n) == CFNumberGetTypeID()) CFNumberGetValue(n, kCFNumberSInt64Type, &v);
    return v > 0 ? (uint64_t)v : 0;
}

// Description and counts dictionaries carry the same keys. Only Wi-Fi + wired bytes count (no loopback,
// cellular or tunnels), matching the old `nettop -t wifi -t wired`.
static void update(struct src *s, CFDictionaryRef d) {
    int32_t pid = (int32_t)num(d, k_pid);
    if (pid > 0 && pid != s->pid) {
        s->pid = pid;
        // Resolve the path while the process is alive: short-lived ones are gone by the next snapshot.
        char path[PROC_PIDPATHINFO_MAXSIZE];
        int n = proc_pidpath(pid, path, sizeof path);
        struct rec *r = rec_for(pid);
        char *p = r && n > 0 && strcmp(r->path, path) ? strdup(path) : NULL;
        if (p) {
            // A different binary on this pid is a reused pid: the old process's closed bytes aren't its.
            if (*r->path) r->rx = r->tx = 0;
            free(r->path);
            r->path = p;
        }
    }
    uint64_t rx = num(d, k_rx_wifi) + num(d, k_rx_wired), tx = num(d, k_tx_wifi) + num(d, k_tx_wired);
    if (rx > s->rx) s->rx = rx;
    if (tx > s->tx) s->tx = tx;
}

static void added(S source, void *unused) {
    (void)unused;
    struct src *s = calloc(1, sizeof *s);
    if (!s) return;
    if ((s->next = srcs)) srcs->prev = s;
    srcs = s;
    set_desc(source, ^(CFDictionaryRef d) { update(s, d); });
    set_counts(source, ^(CFDictionaryRef d) { update(s, d); });
    // Fires after the final counts, so s holds the source's totals.
    set_removed(source, ^{
        struct rec *r = s->pid > 0 ? rec_for(s->pid) : NULL;
        if (r) r->rx += s->rx, r->tx += s->tx;
        if (s->prev) s->prev->next = s->next; else srcs = s->next;
        if (s->next) s->next->prev = s->prev;
        free(s);
    });
}

#define SYM(var, name) if (!(*(void **)&var = dlsym(h, name))) return -2
#define KEY(var, name) { CFStringRef *p = dlsym(h, name); if (!p || !*p) return -2; var = *p; }

// 0 on success, -1 framework missing, -2 symbol missing, -3 manager not created. Call once.
int ov_nstat_start(void) {
    void *h = dlopen("/System/Library/PrivateFrameworks/NetworkStatistics.framework/NetworkStatistics", RTLD_LAZY);
    if (!h) return -1;
    SYM(create, "NStatManagerCreate");
    SYM(add_tcp, "NStatManagerAddAllTCP");
    SYM(add_udp, "NStatManagerAddAllUDP");
    SYM(set_desc, "NStatSourceSetDescriptionBlock");
    SYM(set_counts, "NStatSourceSetCountsBlock");
    SYM(set_removed, "NStatSourceSetRemovedBlock");
    SYM(query_desc, "NStatManagerQueryAllSourcesDescriptions");
    SYM(query_counts, "NStatManagerQueryAllSources");
    KEY(k_pid, "kNStatSrcKeyPID");
    KEY(k_rx_wifi, "kNStatSrcKeyRxWiFiBytes");
    KEY(k_tx_wifi, "kNStatSrcKeyTxWiFiBytes");
    KEY(k_rx_wired, "kNStatSrcKeyRxWiredBytes");
    KEY(k_tx_wired, "kNStatSrcKeyTxWiredBytes");
    q = dispatch_queue_create("overlook.nstat", DISPATCH_QUEUE_SERIAL);
    __block int rc = 0;
    dispatch_sync(q, ^{
        mgr = create(kCFAllocatorDefault, q, ^(S s, void *u) { added(s, u); });
        if (!mgr) { rc = -3; return; }
        add_tcp(mgr);
        add_udp(mgr);
    });
    return rc;
}

// Per-pid cumulative bytes (closed sources + live ones) into out; returns the count, or -1 on timeout.
// A pid that is gone is reported once more with its final totals, then forgotten.
long ov_nstat_snapshot(struct ov_proc *out, size_t cap) {
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    dispatch_retain(done); // released by the completion block, which may outlive a timed-out wait
    // Descriptions refresh pids for sources that predate start; counts then bring every source up to date.
    dispatch_async(q, ^{
        query_desc(mgr, ^{
            query_counts(mgr, ^{ dispatch_semaphore_signal(done); dispatch_release(done); });
        });
    });
    long timed_out = dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC));
    dispatch_release(done);
    if (timed_out) return -1;
    __block long n = 0;
    dispatch_sync(q, ^{
        for (size_t i = 0; i < nrecs; i++) recs[i].live_rx = recs[i].live_tx = 0, recs[i].live = 0;
        for (struct src *s = srcs; s; s = s->next) {
            struct rec *r = s->pid > 0 ? rec_for(s->pid) : NULL;
            if (r) r->live_rx += s->rx, r->live_tx += s->tx, r->live = 1;
        }
        for (size_t i = 0; i < nrecs && (size_t)n < cap;) {
            struct rec *r = &recs[i];
            out[n].pid = r->pid;
            out[n].rx = r->rx + r->live_rx;
            out[n].tx = r->tx + r->live_tx;
            strlcpy(out[n].path, r->path, sizeof out[n].path);
            n++;
            if (!r->live && kill(r->pid, 0) == -1 && errno == ESRCH) {
                free(r->path);
                recs[i] = recs[--nrecs];
            } else {
                i++;
            }
        }
    });
    return n;
}
