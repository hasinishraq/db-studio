import '@testing-library/jest-dom/vitest';

// jsdom has no ResizeObserver, and the resizable panels need one.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
