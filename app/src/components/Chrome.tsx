// The frame around the work area: menu bar, options bar, tool strip,
// photo tabs and status bar.

import { useEffect, useState } from 'react';
import { CROP_RATIOS, FIT, FULL_CROP, centredCrop, formatUsd, useStudio } from '../store';
import type { CompareMode, Tool } from '../store';
import { Button, Icon, Select, Spinner, cx, inputClass } from '../ui';
import type { IconName } from '../ui';

interface MenuItem {
  label: string;
  shortcut?: string;
  disabled?: boolean;
  run: () => void;
}

export function MenuBar() {
  const studio = useStudio();
  const [open, setOpen] = useState<string | null>(null);
  const hasOutput = Boolean(studio.viewedOutput);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(null);
    window.addEventListener('click', close);
    return () => window.removeEventListener('click', close);
  }, [open]);

  const menus: Record<string, (MenuItem | 'divider')[]> = {
    File: [
      { label: 'Open Photos...', shortcut: 'Ctrl+O', run: studio.openPhotos },
      { label: 'Import Style...', run: () => studio.openStyleDialog() },
      { label: 'Export As...', shortcut: 'Ctrl+Shift+E', run: studio.openExport },
      'divider',
      { label: 'Settings...', run: () => studio.setDialog('settings') },
    ],
    Image: [
      { label: 'Generate', shortcut: 'Ctrl+G', disabled: !studio.activeItem || studio.isGenerating, run: studio.generate },
      'divider',
      { label: 'Approve', shortcut: 'A', disabled: !hasOutput, run: () => studio.setStatus('approved') },
      { label: 'Reject', shortcut: 'X', disabled: !hasOutput, run: () => studio.setStatus('rejected') },
      { label: 'Delete Image', shortcut: 'Del', disabled: !hasOutput, run: studio.deleteViewedOutput },
      'divider',
      { label: 'Crop', shortcut: 'C', disabled: !hasOutput, run: studio.startCrop },
      {
        label: 'Remove Crop',
        disabled: !studio.viewedOutput?.crop,
        run: () => {
          const { activeItem, viewedOutput } = studio;
          if (activeItem && viewedOutput) void studio.removeCrop(activeItem.id, viewedOutput.id);
        },
      },
    ],
    View: [
      { label: 'Generated Image Only', shortcut: '1', run: () => studio.setCompare('single') },
      { label: 'Side by Side', shortcut: '2', run: () => studio.setCompare('split') },
      { label: 'Slider', shortcut: '3', run: () => studio.setCompare('slider') },
      'divider',
      { label: 'Fit to Window', shortcut: '0', run: () => studio.setView(FIT) },
    ],
    Help: [
      { label: 'Keyboard Shortcuts', run: () => studio.setDialog('shortcuts') },
      { label: 'Check for Updates...', run: studio.checkForUpdates },
    ],
  };

  return (
    <div className="flex h-7 shrink-0 items-center border-b border-line bg-panel px-1">
      {Object.entries(menus).map(([name, entries]) => (
        <div key={name} className="relative">
          <button
            type="button"
            className={cx('h-7 rounded-sm px-2.5', open === name ? 'bg-raised' : 'hover:bg-raised')}
            onClick={(event) => {
              event.stopPropagation();
              setOpen(open === name ? null : name);
            }}
            onMouseEnter={() => open && setOpen(name)}
          >
            {name}
          </button>
          {open === name && (
            <div className="absolute top-7 left-0 z-30 min-w-56 rounded border border-edge bg-panel py-1 shadow-xl">
              {entries.map((entry, index) =>
                entry === 'divider' ? (
                  <div key={index} className="my-1 border-t border-line" />
                ) : (
                  <button
                    key={entry.label}
                    type="button"
                    disabled={entry.disabled}
                    onClick={entry.run}
                    className="flex w-full items-center justify-between gap-8 px-3 py-1 text-left hover:bg-accent hover:text-white disabled:pointer-events-none disabled:opacity-40"
                  >
                    <span>{entry.label}</span>
                    <span className="text-[11px] opacity-70">{entry.shortcut}</span>
                  </button>
                ),
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

const SHAPES = ['1:1', '4:5', '3:4', '4:3', '3:2', '16:9'];

/** Replaces the options bar while the crop tool is in use, as in Photoshop. */
function CropOptionsBar() {
  const { cropDraft, setCropDraft, applyCrop, cancelCrop } = useStudio();
  if (!cropDraft) return null;
  const { crop, natural } = cropDraft;

  const chooseRatio = (ratioId: string) => {
    const choice = CROP_RATIOS.find((option) => option.id === ratioId)?.ratio ?? null;
    setCropDraft((draft) => {
      if (!draft) return draft;
      if (choice === null || !draft.natural) return { ...draft, ratioId };
      const pixelRatio = choice === 'original' ? draft.natural.width / draft.natural.height : choice;
      return { ...draft, ratioId, crop: centredCrop(pixelRatio, draft.natural) };
    });
  };

  return (
    <div className="flex h-10 shrink-0 items-center gap-3 border-b border-line bg-panel px-3">
      <span className="flex items-center gap-1.5 font-semibold">
        <Icon name="crop" size={14} /> Crop
      </span>
      <span className="text-dim">Shape</span>
      <Select aria-label="Crop shape" value={cropDraft.ratioId} onChange={(event) => chooseRatio(event.target.value)}>
        {CROP_RATIOS.map((option) => (
          <option key={option.id} value={option.id}>
            {option.label}
          </option>
        ))}
      </Select>
      <span className="text-dim tabular-nums" data-crop-size>
        {natural
          ? `${Math.round(crop.width * natural.width)} x ${Math.round(crop.height * natural.height)} px`
          : 'Loading image...'}
      </span>
      <Button
        className="h-6"
        onClick={() => setCropDraft((draft) => (draft ? { ...draft, crop: FULL_CROP, ratioId: 'free' } : draft))}
      >
        Reset
      </Button>
      <span className="ml-auto" />
      <Button className="h-6" onClick={cancelCrop}>
        Cancel (Esc)
      </Button>
      <Button variant="primary" className="h-6" onClick={applyCrop}>
        Apply crop (Enter)
      </Button>
    </div>
  );
}

export function OptionsBar() {
  const studio = useStudio();
  const { library, settings, activeItem, usableStyles, plannedImages, plannedCost, isGenerating } = studio;
  if (studio.cropDraft) return <CropOptionsBar />;
  if (!library || !settings) return <div className="h-10 shrink-0 border-b border-line bg-panel" />;

  const model = library.models.find((candidate) => candidate.id === settings.model);
  const progress = activeItem ? studio.busy[activeItem.id] : undefined;
  const label = (text: string) => <span className="text-dim">{text}</span>;

  let generateLabel = 'Generate';
  if (progress) {
    generateLabel = `Generating ${progress.done}/${progress.total}`;
  } else if (plannedImages > 0) {
    const cost = plannedCost === null ? '' : `  -  about ${formatUsd(plannedCost)}`;
    generateLabel = `Generate ${plannedImages} image${plannedImages === 1 ? '' : 's'}${cost}`;
  }

  return (
    <div className="flex h-10 shrink-0 items-center gap-2 border-b border-line bg-panel px-3">
      <span className={cx('rounded px-2 py-0.5', usableStyles.length > 0 ? 'bg-accent/20 text-ink' : 'bg-field text-dim')}>
        {usableStyles.length === 0
          ? 'No style selected'
          : `${usableStyles.length} style${usableStyles.length === 1 ? '' : 's'}`}
      </span>

      {label('Options')}
      <Select
        aria-label="Options per style"
        title="How many options to generate for each style"
        value={settings.variants}
        onChange={(event) => studio.changeSettings({ variants: Number(event.target.value) })}
      >
        {[1, 2, 3, 4].map((count) => (
          <option key={count}>{count}</option>
        ))}
      </Select>

      {label('Model')}
      <Select
        aria-label="Model"
        title={model?.note}
        value={settings.model}
        onChange={(event) => studio.changeSettings({ model: event.target.value })}
      >
        {library.models.map((candidate) => (
          <option key={candidate.id} value={candidate.id}>
            {candidate.label}
          </option>
        ))}
      </Select>

      {label('Size')}
      <Select
        aria-label="Image size"
        value={settings.imageSize}
        onChange={(event) => studio.changeSettings({ imageSize: event.target.value })}
      >
        {Object.entries(model?.prices ?? {}).map(([size, price]) => (
          <option key={size} value={size}>
            {size} ({formatUsd(price)})
          </option>
        ))}
      </Select>

      {label('Shape')}
      <Select
        aria-label="Image shape"
        value={settings.aspectRatio ?? ''}
        onChange={(event) => studio.changeSettings({ aspectRatio: event.target.value || null })}
      >
        <option value="">Same as photo</option>
        {SHAPES.map((shape) => (
          <option key={shape}>{shape}</option>
        ))}
      </Select>

      <input
        aria-label="Note"
        className={cx(inputClass, 'h-6 min-w-24 flex-1')}
        placeholder="Optional note for the scene, e.g. slightly warmer light"
        value={studio.note}
        onChange={(event) => studio.setNote(event.target.value)}
      />

      <Button variant="primary" disabled={!activeItem || isGenerating} onClick={studio.generate}>
        {isGenerating && <Spinner size={11} />} {generateLabel}
      </Button>
    </div>
  );
}

/** Appears under the options bar when a newer version has been published. */
export function UpdateBanner() {
  const { update, setUpdate, updating, installUpdate, busy } = useStudio();
  if (!update) return null;
  const generating = Object.keys(busy).length > 0;
  return (
    <div className="flex h-9 shrink-0 items-center gap-3 border-b border-line bg-accent/20 px-3">
      {updating ? (
        <>
          <Spinner size={12} />
          <span>Downloading version {update.version}. Jewelry Studio will restart by itself.</span>
        </>
      ) : (
        <>
          <span>
            <strong>Version {update.version} is available.</strong> You have {update.currentVersion}.
            {update.notes && <span className="text-dim"> {update.notes}</span>}
          </span>
          <Button
            variant="primary"
            className="ml-auto h-6"
            disabled={generating}
            title={generating ? 'Wait for the current generation to finish' : undefined}
            onClick={installUpdate}
          >
            Update and restart
          </Button>
          <Button className="h-6" onClick={() => setUpdate(null)}>
            Later
          </Button>
        </>
      )}
    </div>
  );
}

function ToolButton({
  icon,
  title,
  active,
  disabled,
  tone,
  onClick,
}: {
  icon: IconName;
  title: string;
  active?: boolean;
  disabled?: boolean;
  tone?: 'good' | 'bad';
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
      className={cx(
        'flex h-8 w-8 items-center justify-center rounded disabled:opacity-30',
        active ? 'bg-accent text-white' : 'text-ink hover:bg-raised',
        active && tone === 'good' && 'bg-good',
        active && tone === 'bad' && 'bg-bad',
      )}
    >
      <Icon name={icon} />
    </button>
  );
}

export function ToolStrip() {
  const studio = useStudio();
  const { tool, compare, viewedOutput } = studio;
  const noOutput = !viewedOutput;
  const tools: [Tool, IconName, string][] = [
    ['hand', 'hand', 'Hand: drag to move the picture (H)'],
    ['zoom', 'zoom', 'Zoom: click to zoom in, Alt+click to zoom out (Z)'],
  ];
  const modes: [CompareMode, IconName, string][] = [
    ['single', 'single', 'Generated image only (1)'],
    ['split', 'split', 'Reference and generated side by side (2)'],
    ['slider', 'slider', 'Slider between reference and generated (3)'],
  ];
  const rule = <div className="my-1 h-px w-6 bg-edge" />;

  return (
    <div className="flex w-11 shrink-0 flex-col items-center gap-1 border-r border-line bg-panel py-2">
      {tools.map(([id, icon, title]) => (
        <ToolButton key={id} icon={icon} title={title} active={tool === id} onClick={() => studio.setTool(id)} />
      ))}
      <ToolButton
        icon="crop"
        title="Crop the generated image (C)"
        active={tool === 'crop'}
        disabled={noOutput}
        onClick={() => studio.setTool('crop')}
      />
      {rule}
      {modes.map(([id, icon, title]) => (
        <ToolButton key={id} icon={icon} title={title} active={compare === id} onClick={() => studio.setCompare(id)} />
      ))}
      <ToolButton icon="fit" title="Fit to window (0)" onClick={() => studio.setView(FIT)} />
      {rule}
      <ToolButton
        icon="check"
        title="Approve this image (A)"
        tone="good"
        active={viewedOutput?.status === 'approved'}
        disabled={noOutput}
        onClick={() => studio.setStatus('approved')}
      />
      <ToolButton
        icon="cross"
        title="Reject this image (X)"
        tone="bad"
        active={viewedOutput?.status === 'rejected'}
        disabled={noOutput}
        onClick={() => studio.setStatus('rejected')}
      />
      <ToolButton icon="trash" title="Delete this image (Del)" disabled={noOutput} onClick={studio.deleteViewedOutput} />
      {rule}
      <ToolButton icon="export" title="Export As... (Ctrl+Shift+E)" onClick={studio.openExport} />
    </div>
  );
}

export function Tabs() {
  const { items, activeItem, busy, selectItem, removeItem, openPhotos } = useStudio();
  if (items.length === 0) return null;
  return (
    <div className="flex h-8 shrink-0 items-end overflow-x-auto border-b border-line bg-[#232323]">
      {items.map((item) => {
        const active = item.id === activeItem?.id;
        return (
          <div
            key={item.id}
            className={cx(
              'group flex h-7 max-w-52 shrink-0 items-center gap-1.5 border-r border-line pr-1 pl-3',
              active ? 'bg-canvas text-ink' : 'bg-panel text-dim hover:text-ink',
            )}
          >
            <button type="button" className="truncate" title={item.name} onClick={() => selectItem(item.id)}>
              {item.name}
            </button>
            {busy[item.id] && <Spinner size={10} />}
            <button
              type="button"
              aria-label={`Remove ${item.name}`}
              title="Remove this photo"
              className="rounded p-0.5 opacity-50 hover:bg-raised hover:opacity-100"
              onClick={() => removeItem(item)}
            >
              <Icon name="cross" size={11} />
            </button>
          </div>
        );
      })}
      <button
        type="button"
        title="Open more photos (Ctrl+O)"
        aria-label="Open more photos"
        className="flex h-7 w-8 shrink-0 items-center justify-center text-dim hover:text-ink"
        onClick={openPhotos}
      >
        <Icon name="plus" size={14} />
      </button>
    </div>
  );
}

export function StatusBar() {
  const { activeItem, viewedOutput, zoomPercent, items, library, version } = useStudio();
  const approved = items.reduce(
    (count, item) => count + item.outputs.filter((output) => output.status === 'approved').length,
    0,
  );
  return (
    <div className="flex h-6 shrink-0 items-center gap-4 border-t border-line bg-panel px-3 text-[11px] text-dim">
      {activeItem && (
        <>
          <span className="w-12 tabular-nums">{zoomPercent}%</span>
          <span>
            {viewedOutput
              ? `${viewedOutput.styleName} v${viewedOutput.styleVersion}, ${viewedOutput.model}, ${viewedOutput.imageSize}${viewedOutput.crop ? ', cropped' : ''}`
              : `Reference, ${activeItem.width} x ${activeItem.height} px`}
          </span>
        </>
      )}
      <span className="ml-auto">
        {approved} approved for export
        {library && `   |   Prices checked ${library.pricesCheckedOn}`}
        {version && `   |   Version ${version}`}
      </span>
    </div>
  );
}
