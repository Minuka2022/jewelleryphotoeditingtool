// The work area: shows the reference photo and the generated image, with
// zoom, pan and three ways of comparing the two.

import { useCallback, useEffect, useRef, useState } from 'react';
import type { PointerEvent as ReactPointerEvent, WheelEvent as ReactWheelEvent } from 'react';
import { api } from '../api';
import { FIT, useStudio } from '../store';
import type { View } from '../store';
import { Button, Spinner, cx } from '../ui';

const MIN_ZOOM = 0.25;
const MAX_ZOOM = 16;
const PANE_PADDING = 16;

/** Zooms by `factor`, keeping the point under the cursor where it is. */
function zoomAt(view: View, factor: number, clientX: number, clientY: number, pane: DOMRect): View {
  const zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, view.zoom * factor));
  const ratio = zoom / view.zoom;
  const cursorX = clientX - pane.left - pane.width / 2;
  const cursorY = clientY - pane.top - pane.height / 2;
  return {
    zoom,
    x: cursorX - ratio * (cursorX - view.x),
    y: cursorY - ratio * (cursorY - view.y),
  };
}

function paneUnder(target: EventTarget): DOMRect | null {
  return (target as HTMLElement).closest('[data-pane]')?.getBoundingClientRect() ?? null;
}

function Picture({
  src,
  alt,
  view,
  onFit,
}: {
  src: string;
  alt: string;
  view: View;
  /** Reports how much the picture is scaled down to fit its pane. */
  onFit?: (scale: number) => void;
}) {
  const ref = useRef<HTMLImageElement>(null);

  useEffect(() => {
    const image = ref.current;
    const pane = image?.parentElement;
    if (!image || !pane || !onFit) return;
    const measure = () => {
      if (image.naturalWidth === 0) return;
      const width = pane.clientWidth - PANE_PADDING * 2;
      const height = pane.clientHeight - PANE_PADDING * 2;
      onFit(Math.min(1, width / image.naturalWidth, height / image.naturalHeight));
    };
    const observer = new ResizeObserver(measure);
    observer.observe(pane);
    image.addEventListener('load', measure);
    measure();
    return () => {
      observer.disconnect();
      image.removeEventListener('load', measure);
    };
  }, [src, onFit]);

  return (
    <img
      ref={ref}
      src={src}
      alt={alt}
      draggable={false}
      className="max-h-full max-w-full shadow-[0_0_0_1px_rgba(255,255,255,0.06)]"
      style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})` }}
    />
  );
}

function Tag({ children, side = 'left' }: { children: string; side?: 'left' | 'right' }) {
  return (
    <span
      className={cx(
        'pointer-events-none absolute top-2 z-10 rounded bg-black/60 px-2 py-0.5 text-[10px] tracking-wider text-dim uppercase',
        side === 'left' ? 'left-2' : 'right-2',
      )}
    >
      {children}
    </span>
  );
}

const paneClass = 'relative flex min-h-0 min-w-0 flex-1 items-center justify-center overflow-hidden p-4';

function EmptyState() {
  const { openPhotos, settings, setDialog } = useStudio();
  return (
    <div className="flex flex-1 items-center justify-center p-10">
      <div className="flex w-[440px] flex-col items-center gap-3 rounded-lg border border-dashed border-edge px-8 py-12 text-center">
        <p className="text-[15px] font-semibold">Drop jewelry photos here</p>
        <p className="text-dim">JPG, PNG or WebP. Your original files are never changed.</p>
        <Button variant="primary" onClick={openPhotos}>
          Open Photos...
        </Button>
        {settings && !settings.hasApiKey && (
          <p className="mt-3 text-dim">
            Before generating, add your Gemini API key in{' '}
            <button type="button" className="text-accent underline" onClick={() => setDialog('settings')}>
              Settings
            </button>
            .
          </p>
        )}
      </div>
    </div>
  );
}

export function Canvas() {
  const { activeItem, viewedOutput, compare, tool, view, setView, setZoomPercent, busy } = useStudio();
  const [divider, setDivider] = useState(50);
  const [fitScale, setFitScale] = useState(1);
  const [panning, setPanning] = useState(false);
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null);

  useEffect(() => {
    setZoomPercent(Math.round(fitScale * view.zoom * 100));
  }, [fitScale, view.zoom, setZoomPercent]);

  const onFit = useCallback((scale: number) => setFitScale(scale), []);

  if (!activeItem) return <EmptyState />;

  const onWheel = (event: ReactWheelEvent) => {
    const pane = paneUnder(event.target);
    if (pane) setView(zoomAt(view, event.deltaY < 0 ? 1.15 : 1 / 1.15, event.clientX, event.clientY, pane));
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    drag.current = { x: event.clientX, y: event.clientY, moved: false };
    event.currentTarget.setPointerCapture(event.pointerId);
    setPanning(true);
  };

  const onPointerMove = (event: ReactPointerEvent) => {
    const start = drag.current;
    if (!start) return;
    const dx = event.clientX - start.x;
    const dy = event.clientY - start.y;
    if (!start.moved && Math.hypot(dx, dy) < 3) return;
    drag.current = { x: event.clientX, y: event.clientY, moved: true };
    setView({ ...view, x: view.x + dx, y: view.y + dy });
  };

  const onPointerUp = (event: ReactPointerEvent) => {
    const start = drag.current;
    drag.current = null;
    setPanning(false);
    // A click without dragging uses the zoom tool; Alt zooms out.
    const pane = paneUnder(event.target);
    if (start && !start.moved && tool === 'zoom' && pane) {
      setView(zoomAt(view, event.altKey ? 1 / 1.5 : 1.5, event.clientX, event.clientY, pane));
    }
  };

  const moveDivider = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
    const pane = event.currentTarget.parentElement!.getBoundingClientRect();
    setDivider(Math.min(100, Math.max(0, ((event.clientX - pane.left) / pane.width) * 100)));
  };

  const reference = api.fileSrc(activeItem.reference);
  const output = viewedOutput ? api.fileSrc(viewedOutput.path) : null;
  const outputLabel = viewedOutput ? `Generated: ${viewedOutput.styleName}` : '';
  const progress = busy[activeItem.id];
  const cursor = tool === 'zoom' ? 'cursor-zoom-in' : panning ? 'cursor-grabbing' : 'cursor-grab';

  return (
    <div
      className={cx('relative flex min-h-0 flex-1 bg-canvas', cursor)}
      onWheel={onWheel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onDoubleClick={() => setView(FIT)}
    >
      {!output && (
        <div data-pane className={paneClass}>
          <Tag>Reference</Tag>
          <Picture src={reference} alt="Reference photo" view={view} onFit={onFit} />
          {activeItem.outputs.length === 0 && !progress && (
            <p className="pointer-events-none absolute bottom-4 rounded bg-black/60 px-3 py-1.5 text-dim">
              Choose one or more styles on the right, then press Generate.
            </p>
          )}
        </div>
      )}

      {output && compare === 'single' && (
        <div data-pane className={paneClass}>
          <Tag>{outputLabel}</Tag>
          <Picture src={output} alt="Generated image" view={view} onFit={onFit} />
        </div>
      )}

      {output && compare === 'split' && (
        <>
          <div data-pane className={paneClass}>
            <Tag>Reference</Tag>
            <Picture src={reference} alt="Reference photo" view={view} />
          </div>
          <div className="w-px bg-line" />
          <div data-pane className={paneClass}>
            <Tag>{outputLabel}</Tag>
            <Picture src={output} alt="Generated image" view={view} onFit={onFit} />
          </div>
        </>
      )}

      {output && compare === 'slider' && (
        <div data-pane className="relative min-h-0 min-w-0 flex-1 overflow-hidden">
          <Tag>Reference</Tag>
          <Tag side="right">{outputLabel}</Tag>
          <div className="absolute inset-0 flex items-center justify-center p-4">
            <Picture src={reference} alt="Reference photo" view={view} />
          </div>
          <div
            className="absolute inset-0 flex items-center justify-center p-4"
            style={{ clipPath: `inset(0 0 0 ${divider}%)` }}
          >
            <Picture src={output} alt="Generated image" view={view} onFit={onFit} />
          </div>
          <div
            role="slider"
            aria-label="Comparison divider"
            aria-valuenow={Math.round(divider)}
            aria-valuemin={0}
            aria-valuemax={100}
            tabIndex={-1}
            className="absolute top-0 bottom-0 z-10 -ml-2 flex w-4 cursor-ew-resize justify-center"
            style={{ left: `${divider}%` }}
            onPointerDown={(event) => {
              event.stopPropagation();
              event.currentTarget.setPointerCapture(event.pointerId);
            }}
            onPointerMove={moveDivider}
            onPointerUp={(event) => event.stopPropagation()}
            onDoubleClick={(event) => event.stopPropagation()}
          >
            <div className="h-full w-0.5 bg-white/85" />
            <div className="absolute top-1/2 h-8 w-3 -translate-y-1/2 rounded bg-white shadow" />
          </div>
        </div>
      )}

      {progress && (
        <div className="pointer-events-none absolute top-3 left-1/2 z-20 flex -translate-x-1/2 items-center gap-2 rounded-full border border-edge bg-panel/95 px-3 py-1.5 shadow-lg">
          <Spinner />
          Generating {Math.min(progress.done + 1, progress.total)} of {progress.total}...
        </div>
      )}
    </div>
  );
}
