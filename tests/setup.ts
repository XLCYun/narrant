import "@testing-library/jest-dom/vitest";

// base-ui portals may reference ResizeObserver, which jsdom does not provide.
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
if (!("ResizeObserver" in globalThis)) {
  globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;
}

if (!window.matchMedia) {
  window.matchMedia = ((query: string) => ({ media: query, matches: false, onchange: null, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent() { return false; } })) as typeof window.matchMedia;
}

const storedValues = new Map<string, string>();
Object.defineProperty(window, "localStorage", { configurable: true, value: { getItem: (key: string) => storedValues.get(key) ?? null, setItem: (key: string, value: string) => storedValues.set(key, value), removeItem: (key: string) => storedValues.delete(key), clear: () => storedValues.clear() } });
