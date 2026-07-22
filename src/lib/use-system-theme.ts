import { useLayoutEffect, useState } from "react";

const SYSTEM_DARK_THEME_QUERY = "(prefers-color-scheme: dark)";

function systemUsesDarkTheme() {
  return window.matchMedia(SYSTEM_DARK_THEME_QUERY).matches;
}

export function useSystemTheme() {
  const [isDark, setIsDark] = useState(systemUsesDarkTheme);

  useLayoutEffect(() => {
    const mediaQuery = window.matchMedia(SYSTEM_DARK_THEME_QUERY);
    const syncTheme = (dark: boolean) => {
      document.documentElement.classList.toggle("dark", dark);
      setIsDark(dark);
    };
    const handleChange = (event: MediaQueryListEvent) => syncTheme(event.matches);

    syncTheme(mediaQuery.matches);
    mediaQuery.addEventListener("change", handleChange);
    return () => mediaQuery.removeEventListener("change", handleChange);
  }, []);

  return isDark ? "vs-dark" : "light";
}
