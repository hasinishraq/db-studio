import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ Channel: class {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../bindings', () => ({ commands: {}, events: {} }));
vi.mock('../context/Studio', () => ({ newId: () => 'id', useStudio: () => ({}) }));

import { edgeEnds } from '../components/workbench/ErDiagram';
import { formatBytes } from '../components/workbench/TableTab';

describe('diagram edges', () => {
  it('reads both ends of a relationship curve', () => {
    expect(edgeEnds('M280,50 C320,50 360,90 400,90')).toEqual({ start: { x: 280, y: 50 }, end: { x: 400, y: 90 } });
    expect(edgeEnds('M-10.5,4 C0,0 0,0 12.25,-3')).toEqual({ start: { x: -10.5, y: 4 }, end: { x: 12.25, y: -3 } });
  });

  it('gives up on a path it cannot read', () => {
    expect(edgeEnds('M1')).toBeNull();
  });
});

describe('table sizes', () => {
  it('uses the largest unit that keeps the number above 1', () => {
    expect(formatBytes(512)).toBe('512 bytes');
    expect(formatBytes(2048)).toBe('2.0 KB');
    expect(formatBytes(2_500_000)).toBe('2.4 MB');
    expect(formatBytes(50 * 1024 * 1024)).toBe('50 MB');
  });
});
