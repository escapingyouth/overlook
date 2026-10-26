import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

export type Appearance = "system" | "light" | "dark";
const KEY = "overlook.appearance";
const media = window.matchMedia("(prefers-color-scheme: dark)");

export const getAppearance = (): Appearance => (localStorage.getItem(KEY) as Appearance) || "system";

function apply(pref: Appearance) {
  const dark = pref === "dark" || (pref === "system" && media.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  getCurrentWindow().setTheme(pref === "system" ? null : pref).catch(() => {});
}

export function useAppearance(): [Appearance, (a: Appearance) => void] {
  const [pref, setPref] = useState(getAppearance);
  useEffect(() => {
    apply(pref);
    const onSystem = () => apply(getAppearance());
    const onStorage = (e: StorageEvent) => e.key === KEY && setPref(getAppearance());
    media.addEventListener("change", onSystem);
    window.addEventListener("storage", onStorage);
    return () => {
      media.removeEventListener("change", onSystem);
      window.removeEventListener("storage", onStorage);
    };
  }, [pref]);
  return [pref, (a) => {
    localStorage.setItem(KEY, a);
    setPref(a);
  }];
}
