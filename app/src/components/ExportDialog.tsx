// File > Export As: format, quality, size and resampling, with the result's
// pixel size and file size shown before anything is written.

import { useEffect, useMemo, useState } from 'react';
import { api, errorText } from '../api';
import { useStudio } from '../store';
import type { ExportEstimate, ExportFormat, ExportOptions, OutputRef, Resample } from '../types';
import { Button, Field, Modal, Select, Spinner, cx, inputClass } from '../ui';

const SAVED_OPTIONS = 'jewelry-studio.export-options';

const DEFAULTS: ExportOptions = { format: 'jpeg', quality: 92, width: null, resample: 'lanczos', dpi: 72 };

const FORMATS: [ExportFormat, string, string][] = [
  ['jpeg', 'JPEG', 'Smallest files. Best for online stores and email.'],
  ['png', 'PNG', 'Lossless: every pixel is kept exactly. Large files.'],
  ['webp', 'WebP', 'Lossless and smaller than PNG. Not every program opens it.'],
];

const QUALITY_PRESETS: [string, number][] = [
  ['Low', 30],
  ['Medium', 60],
  ['High', 80],
  ['Very high', 92],
  ['Maximum', 100],
];

const RESAMPLING: [Resample, string][] = [
  ['lanczos', 'Lanczos: sharpest, best for making smaller'],
  ['bicubic', 'Bicubic: smooth, best for making larger'],
  ['bilinear', 'Bilinear: soft'],
  ['nearest', 'Nearest neighbour: hard edges'],
];

const DENSITIES: [number, string][] = [
  [72, '72 ppi (screen)'],
  [150, '150 ppi'],
  [300, '300 ppi (print)'],
];

const MIN_EDGE = 16;
const MAX_EDGE = 10000;

/** The last used settings, apart from the size, which belongs to one image. */
function savedOptions(): ExportOptions {
  try {
    const saved = JSON.parse(localStorage.getItem(SAVED_OPTIONS) ?? 'null');
    if (saved && typeof saved === 'object') return { ...DEFAULTS, ...saved, width: null };
  } catch {
    // Storage can be unavailable; the defaults are fine.
  }
  return DEFAULTS;
}

function saveOptions(options: ExportOptions) {
  try {
    localStorage.setItem(SAVED_OPTIONS, JSON.stringify({ ...options, width: null }));
  } catch {
    // Not worth bothering the user about.
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** A number box that only applies its value on Enter or when it loses focus. */
function NumberField({
  label,
  value,
  suffix,
  onCommit,
}: {
  label: string;
  value: number | null;
  suffix: string;
  onCommit: (value: number) => void;
}) {
  const [text, setText] = useState(value === null ? '' : String(value));
  useEffect(() => setText(value === null ? '' : String(value)), [value]);
  const commit = () => {
    const parsed = Math.round(Number(text));
    if (Number.isFinite(parsed) && parsed > 0) onCommit(parsed);
    else setText(value === null ? '' : String(value));
  };
  return (
    <label className="flex items-center gap-1.5">
      <span className="w-12 text-[11px] text-dim">{label}</span>
      <input
        aria-label={label}
        inputMode="numeric"
        className={cx(inputClass, 'w-20 text-right tabular-nums')}
        value={text}
        disabled={value === null}
        onChange={(event) => setText(event.target.value.replace(/[^\d]/g, ''))}
        onBlur={commit}
        onKeyDown={(event) => event.key === 'Enter' && event.currentTarget.blur()}
      />
      <span className="text-[11px] text-dim">{suffix}</span>
    </label>
  );
}

function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: [T, string][];
  onChange: (value: T) => void;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="flex gap-1.5">
      {options.map(([id, text]) => (
        <button
          key={id}
          type="button"
          role="radio"
          aria-checked={value === id}
          onClick={() => onChange(id)}
          className={cx(
            'rounded-full border px-2.5 py-0.5',
            value === id ? 'border-accent bg-accent/25 text-ink' : 'border-edge text-dim hover:text-ink',
          )}
        >
          {text}
        </button>
      ))}
    </div>
  );
}

export function ExportDialog() {
  const { items, activeItem, viewedOutput, approvedOutputs, setDialog, notify } = useStudio();
  const close = () => setDialog(null);

  const [scope, setScope] = useState<'current' | 'approved'>(viewedOutput ? 'current' : 'approved');
  const [options, setOptions] = useState<ExportOptions>(savedOptions);
  const [estimate, setEstimate] = useState<ExportEstimate | null>(null);
  const [estimating, setEstimating] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  const current: OutputRef | null =
    activeItem && viewedOutput ? { itemId: activeItem.id, outputId: viewedOutput.id } : null;
  const selections = scope === 'current' && current ? [current] : approvedOutputs;
  const sample = selections[0] ?? null;
  const sampleOutput = useMemo(
    () => items.find((item) => item.id === sample?.itemId)?.outputs.find((output) => output.id === sample?.outputId),
    [items, sample?.itemId, sample?.outputId],
  );
  const change = (patch: Partial<ExportOptions>) => setOptions((previous) => ({ ...previous, ...patch }));

  // Work out the size shortly after the settings stop changing.
  useEffect(() => {
    if (!sample) {
      setEstimate(null);
      return;
    }
    let live = true;
    setEstimating(true);
    const timer = setTimeout(() => {
      api
        .estimateExport(sample, options)
        .then((result) => {
          if (!live) return;
          setEstimate(result);
          setProblem(null);
        })
        .catch((error) => live && setProblem(errorText(error)))
        .finally(() => live && setEstimating(false));
    }, 300);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [sample?.itemId, sample?.outputId, options]); // eslint-disable-line react-hooks/exhaustive-deps

  // A new image keeps its own size.
  useEffect(() => change({ width: null }), [sample?.itemId, sample?.outputId]);

  const source = estimate ? { width: estimate.sourceWidth, height: estimate.sourceHeight } : null;
  const width = source ? (options.width ?? source.width) : null;
  const height = source && width ? Math.max(1, Math.round((width * source.height) / source.width)) : null;
  const percent = source && width ? Math.round((width / source.width) * 100) : null;
  const setWidth = (value: number) => {
    if (!source) return;
    const clamped = Math.min(MAX_EDGE, Math.max(MIN_EDGE, value));
    change({ width: clamped === source.width ? null : clamped });
  };
  const enlarging = Boolean(source && width && width > source.width);
  const isJpeg = options.format === 'jpeg';
  const preset = QUALITY_PRESETS.find(([, quality]) => quality === options.quality)?.[0] ?? 'Custom';

  const run = async () => {
    setExporting(true);
    try {
      const result = await api.exportImages(selections, options);
      if (!result) return;
      saveOptions(options);
      notify('info', `Exported ${result.count} image${result.count === 1 ? '' : 's'} to ${result.where}`);
      close();
    } catch (error) {
      setProblem(errorText(error));
    } finally {
      setExporting(false);
    }
  };

  const sizeLine = estimate
    ? `${estimate.width} x ${estimate.height} px, ${formatBytes(estimate.bytes)}`
    : estimating
      ? 'Working out the size...'
      : '';

  return (
    <Modal
      title="Export As"
      width="w-[880px]"
      onClose={close}
      footer={
        <>
          <span className="mr-auto flex items-center gap-1.5 self-center text-[11px] text-dim">
            {estimating && <Spinner size={10} />}
            <span data-export-size>{selections.length > 1 ? `First image: ${sizeLine}` : sizeLine}</span>
          </span>
          <Button onClick={close}>Cancel</Button>
          <Button variant="primary" disabled={exporting || selections.length === 0 || !estimate} onClick={run}>
            {exporting ? 'Exporting...' : `Export ${selections.length} image${selections.length === 1 ? '' : 's'}...`}
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-[1fr_330px] gap-4">
        <div className="flex h-[440px] items-center justify-center rounded border border-line bg-canvas p-3">
          {sampleOutput ? (
            <img
              src={api.fileSrc(sampleOutput.path)}
              alt="Image to export"
              className="max-h-full max-w-full object-contain shadow-[0_0_0_1px_rgba(255,255,255,0.06)]"
            />
          ) : (
            <p className="text-dim">No image chosen.</p>
          )}
        </div>

        <div className="space-y-3">
          <Field label="Export">
            <Segmented
              label="What to export"
              value={scope}
              onChange={setScope}
              options={[
                ...(current ? ([['current', 'This image']] as ['current', string][]) : []),
                ['approved', `All approved (${approvedOutputs.length})`],
              ]}
            />
          </Field>

          <Field label="Format" hint={FORMATS.find(([id]) => id === options.format)?.[2]}>
            <Segmented
              label="File format"
              value={options.format}
              onChange={(format) => change({ format })}
              options={FORMATS.map(([id, text]) => [id, text])}
            />
          </Field>

          {isJpeg && (
            <Field
              label="Quality"
              hint="At 90 and above, full colour detail is kept (4:4:4), as Photoshop does at its highest settings."
            >
              <div className="flex items-center gap-2">
                <Select
                  aria-label="Quality preset"
                  value={preset}
                  onChange={(event) => {
                    const chosen = QUALITY_PRESETS.find(([name]) => name === event.target.value);
                    if (chosen) change({ quality: chosen[1] });
                  }}
                >
                  {QUALITY_PRESETS.map(([name]) => (
                    <option key={name}>{name}</option>
                  ))}
                  {preset === 'Custom' && <option>Custom</option>}
                </Select>
                <input
                  type="range"
                  aria-label="Quality"
                  min={1}
                  max={100}
                  value={options.quality}
                  onChange={(event) => change({ quality: Number(event.target.value) })}
                  className="flex-1 accent-[#2f8cf0]"
                />
                <span className="w-7 text-right tabular-nums" data-quality>
                  {options.quality}
                </span>
              </div>
            </Field>
          )}

          <Field label="Image size" hint={selections.length > 1 ? 'The width applies to every image; each keeps its own proportions.' : undefined}>
            <div className="space-y-1.5">
              <NumberField label="Width" value={width} suffix="px" onCommit={setWidth} />
              <NumberField
                label="Height"
                value={height}
                suffix="px"
                onCommit={(value) => source && setWidth(Math.round((value * source.width) / source.height))}
              />
              <NumberField
                label="Scale"
                value={percent}
                suffix="%"
                onCommit={(value) => source && setWidth(Math.round((source.width * value) / 100))}
              />
              <div className="flex items-center gap-2 pt-0.5">
                <Button className="h-6" disabled={options.width === null} onClick={() => change({ width: null })}>
                  Original size
                </Button>
                {source && (
                  <span className="text-[11px] text-dim">
                    {source.width} x {source.height} px{sampleOutput?.crop ? ' after crop' : ''}
                  </span>
                )}
              </div>
              {enlarging && (
                <p className="text-[11px] text-[#e3c16a]">Enlarging adds pixels, not detail. The image may look soft.</p>
              )}
            </div>
          </Field>

          <Field label="Resample">
            <Select
              aria-label="Resample"
              className="w-full"
              value={options.resample}
              disabled={options.width === null}
              onChange={(event) => change({ resample: event.target.value as Resample })}
            >
              {RESAMPLING.map(([id, text]) => (
                <option key={id} value={id}>
                  {text}
                </option>
              ))}
            </Select>
          </Field>

          {isJpeg && (
            <Field label="Resolution" hint="Printers read this number; screens ignore it.">
              <Select
                aria-label="Resolution"
                className="w-full"
                value={options.dpi}
                onChange={(event) => change({ dpi: Number(event.target.value) })}
              >
                {DENSITIES.map(([dpi, text]) => (
                  <option key={dpi} value={dpi}>
                    {text}
                  </option>
                ))}
              </Select>
            </Field>
          )}

          {problem && <p className="text-bad">{problem}</p>}
        </div>
      </div>
    </Modal>
  );
}
