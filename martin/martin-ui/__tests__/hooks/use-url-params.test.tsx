import { act, renderHook } from '@testing-library/react';
import { useURLParams } from '@/hooks/use-url-params';

// Mock window.history and location
const mockReplaceState = jest.fn();
const mockAddEventListener = jest.fn();
const mockRemoveEventListener = jest.fn();

Object.defineProperty(window, 'history', {
  value: {
    replaceState: mockReplaceState,
  },
  writable: true,
});

Object.defineProperty(window, 'addEventListener', {
  value: mockAddEventListener,
  writable: true,
});

Object.defineProperty(window, 'removeEventListener', {
  value: mockRemoveEventListener,
  writable: true,
});

describe('useURLParams', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('should initialize with default params', () => {
    const { result } = renderHook(() =>
      useURLParams({
        downloadSprite: undefined,
        inspectTile: undefined,
        previewSprite: undefined,
        style: undefined,
        styleGuide: undefined,
        tab: 'tiles',
      }),
    );

    expect(result.current.params).toEqual({
      downloadSprite: undefined,
      inspectTile: undefined,
      previewSprite: undefined,
      style: undefined,
      styleGuide: undefined,
      tab: 'tiles',
    });
  });

  it('should update a single param', () => {
    const { result } = renderHook(() =>
      useURLParams({
        inspectTile: undefined,
        style: undefined,
        tab: 'tiles',
      }),
    );

    act(() => {
      result.current.updateParam('tab', 'styles');
    });

    expect(result.current.params).toEqual({
      inspectTile: undefined,
      style: undefined,
      tab: 'styles',
    });
  });

  it('should update multiple params', () => {
    const { result } = renderHook(() =>
      useURLParams({
        search: '',
        style: undefined,
        tab: 'tiles',
      }),
    );

    act(() => {
      result.current.updateParams({
        style: 'example',
        tab: 'styles',
      });
    });

    expect(result.current.params).toEqual({
      search: '',
      style: 'example',
      tab: 'styles',
    });
  });

  it('should set param to undefined to close dialogs', () => {
    const { result } = renderHook(() =>
      useURLParams({
        inspectTile: 'some-tile',
        previewSprite: 'some-sprite',
        style: 'example',
        tab: 'tiles',
      }),
    );

    // Close tile inspection dialog
    act(() => {
      result.current.updateParam('inspectTile', undefined);
    });

    expect(result.current.params).toEqual({
      inspectTile: undefined,
      previewSprite: 'some-sprite',
      style: 'example',
      tab: 'tiles',
    });

    // Close sprite preview dialog
    act(() => {
      result.current.updateParam('previewSprite', undefined);
    });

    expect(result.current.params).toEqual({
      inspectTile: undefined,
      previewSprite: undefined,
      style: 'example',
      tab: 'tiles',
    });
  });

  it('should add popstate event listener on mount', () => {
    renderHook(() =>
      useURLParams({
        style: undefined,
        tab: 'tiles',
      }),
    );

    expect(mockAddEventListener).toHaveBeenCalledWith('popstate', expect.any(Function));
  });

  it('should remove popstate event listener on unmount', () => {
    const { unmount } = renderHook(() =>
      useURLParams({
        style: undefined,
        tab: 'tiles',
      }),
    );

    unmount();

    expect(mockRemoveEventListener).toHaveBeenCalledWith('popstate', expect.any(Function));
  });
});
