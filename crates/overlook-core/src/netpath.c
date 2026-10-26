// The interface behind the default route when macOS marks that route expensive: cellular-backed,
// which on a Mac means a phone hotspot (Wi-Fi or USB). Network.framework's public C API.
#include <Network/Network.h>
#include <net/if.h>
#include <string.h>

static dispatch_queue_t q;
static char expensive[IFNAMSIZ]; // empty when the default route isn't expensive; only touched on q

void ov_expensive_interface(char *out, size_t cap) {
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        q = dispatch_queue_create("overlook.netpath", DISPATCH_QUEUE_SERIAL);
        dispatch_semaphore_t first = dispatch_semaphore_create(0);
        __block int fresh = 1;
        nw_path_monitor_t m = nw_path_monitor_create(); // never released: watches for the app's lifetime
        nw_path_monitor_set_queue(m, q);
        nw_path_monitor_set_update_handler(m, ^(nw_path_t path) {
            expensive[0] = 0;
            if (nw_path_get_status(path) == nw_path_status_satisfied && nw_path_is_expensive(path))
                nw_path_enumerate_interfaces(path, ^bool(nw_interface_t i) {
                    strlcpy(expensive, nw_interface_get_name(i), sizeof expensive);
                    return false; // the first interface is the one in use
                });
            if (fresh) fresh = 0, dispatch_semaphore_signal(first);
        });
        nw_path_monitor_start(m);
        // So the first answer is real rather than "not yet known"; the update usually lands in milliseconds.
        dispatch_semaphore_wait(first, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC));
    });
    dispatch_sync(q, ^{ strlcpy(out, expensive, cap); });
}
