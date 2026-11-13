import '@testing-library/jest-dom/vitest';

// jsdom has no ResizeObserver, and the resizable panels need one.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// jsdom does not scroll, and cmdk scrolls the picked command into view.
Element.prototype.scrollIntoView ??= function scrollIntoView() {};
