// The right-hand column (Properties, Styles, History) and the filmstrip of
// generated images under the work area.

import { useState } from 'react';
import type { ReactNode } from 'react';
import { api } from '../api';
import { formatUsd, styleFits, useStudio } from '../store';
import type { ItemInfo, OutputInfo, StyleInfo, Tone } from '../types';
import { Icon, Select, Spinner, cx, inputClass } from '../ui';

function Section({
  title,
  action,
  className,
  children,
}: {
  title: string;
  action?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={cx('flex min-h-0 flex-col border-b border-line', className)}>
      <header className="flex h-7 shrink-0 items-center justify-between bg-[#333] px-3">
        <h2 className="text-[11px] font-semibold tracking-wider uppercase">{title}</h2>
        {action}
      </header>
      {children}
    </section>
  );
}

function Properties({ item }: { item: ItemInfo }) {
  const { library, updateItem } = useStudio();
  const [name, setName] = useState(item.name);
  const commit = () => {
    if (name.trim() && name.trim() !== item.name) void updateItem(item.id, { name: name.trim() });
    else setName(item.name);
  };

  return (
    <div className="flex gap-3 p-3">
      <img
        src={api.fileSrc(item.reference)}
        alt="Reference photo"
        className="h-20 w-20 shrink-0 rounded border border-edge bg-canvas object-contain"
      />
      <div className="min-w-0 flex-1 space-y-2">
        <input
          aria-label="Photo name"
          className={inputClass}
          value={name}
          onChange={(event) => setName(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => event.key === 'Enter' && event.currentTarget.blur()}
        />
        <Select
          aria-label="Jewelry type"
          className="w-full"
          value={item.category}
          onChange={(event) => updateItem(item.id, { category: event.target.value })}
        >
          {library?.categories.map((category) => (
            <option key={category.id} value={category.id}>
              {category.label}
            </option>
          ))}
        </Select>
        <p className="text-[11px] text-dim">
          {item.width} x {item.height} px, {item.outputs.length} generated
        </p>
      </div>
    </div>
  );
}

const swatches: Record<Tone, string> = {
  bright: 'linear-gradient(135deg, #f4f2ee, #c9c4bb)',
  neutral: 'linear-gradient(135deg, #9a968f, #5e5b56)',
  dark: 'linear-gradient(135deg, #3d3d44, #0c0c0e)',
};

function StyleCard({ style, item }: { style: StyleInfo; item: ItemInfo | null }) {
  const { library, selectedStyles, toggleStyle, removeStyle } = useStudio();
  const selected = selectedStyles.includes(style.id);
  const fits = !item || styleFits(style, item.category);
  const categoryLabel = library?.categories.find((category) => category.id === item?.category)?.label;

  return (
    <div className={cx('group relative', !fits && 'opacity-35')}>
      <button
        type="button"
        aria-pressed={selected}
        title={fits ? style.description || style.name : `Not set up for ${categoryLabel ?? 'this type'}`}
        onClick={() => toggleStyle(style.id)}
        className={cx(
          'block w-full overflow-hidden rounded border-2 bg-field text-left',
          selected ? 'border-accent' : 'border-transparent hover:border-edge',
        )}
      >
        {style.thumbnail ? (
          <img src={api.fileSrc(style.thumbnail)} alt="" className="h-16 w-full object-cover" />
        ) : (
          <div className="h-16 w-full" style={{ background: swatches[style.tone] }} />
        )}
        <span className="flex items-center gap-1 px-1.5 py-1">
          <span
            className={cx(
              'flex h-3 w-3 shrink-0 items-center justify-center rounded-sm border',
              selected ? 'border-accent bg-accent text-white' : 'border-edge',
            )}
          >
            {selected && <Icon name="check" size={9} />}
          </span>
          <span className="truncate text-[11px]">{style.name}</span>
        </span>
      </button>
      {!style.builtin && (
        <button
          type="button"
          aria-label={`Delete the style ${style.name}`}
          title="Delete this style"
          onClick={() => removeStyle(style)}
          className="absolute top-1 right-1 hidden rounded bg-black/70 p-1 text-ink group-hover:block hover:text-bad"
        >
          <Icon name="trash" size={11} />
        </button>
      )}
    </div>
  );
}

function History({ item }: { item: ItemInfo }) {
  const { viewOutput, viewedOutput } = useStudio();
  const runs = new Map<string, OutputInfo[]>();
  for (const output of item.outputs) {
    runs.set(output.runId, [...(runs.get(output.runId) ?? []), output]);
  }
  if (runs.size === 0) return <p className="p-3 text-dim">Nothing generated yet.</p>;

  return (
    <ul className="overflow-y-auto py-1">
      {[...runs.entries()].reverse().map(([runId, outputs]) => {
        const styles = [...new Set(outputs.map((output) => output.styleName))].join(', ');
        const cost = outputs.reduce((sum, output) => sum + (output.costUsd ?? 0), 0);
        const time = new Date(outputs[0].createdAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        return (
          <li key={runId}>
            <button
              type="button"
              onClick={() => viewOutput(item.id, outputs[0].id)}
              className={cx(
                'flex w-full items-baseline justify-between gap-2 px-3 py-1 text-left hover:bg-raised',
                viewedOutput?.runId === runId && 'bg-raised',
              )}
            >
              <span className="truncate">{styles}</span>
              <span className="shrink-0 text-[11px] text-dim">
                {outputs.length} img, {time}
                {cost > 0 && `, ${formatUsd(cost)}`}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

export function SidePanels() {
  const { library, activeItem, setDialog } = useStudio();
  return (
    <aside className="flex w-72 shrink-0 flex-col border-l border-line bg-panel">
      <Section title="Properties">
        {activeItem ? <Properties key={activeItem.id} item={activeItem} /> : <p className="p-3 text-dim">No photo open.</p>}
      </Section>

      <Section
        title="Styles"
        className="flex-1"
        action={
          <button
            type="button"
            className="flex items-center gap-1 text-[11px] text-accent hover:brightness-125"
            onClick={() => setDialog('import-style')}
          >
            <Icon name="plus" size={11} /> Import style
          </button>
        }
      >
        <div className="grid grid-cols-2 content-start gap-2 overflow-y-auto p-2">
          {library?.styles.map((style) => <StyleCard key={style.id} style={style} item={activeItem} />)}
        </div>
      </Section>

      <Section title="History" className="h-44">
        {activeItem ? <History item={activeItem} /> : <p className="p-3 text-dim">No photo open.</p>}
      </Section>
    </aside>
  );
}

function Thumb({
  src,
  label,
  selected,
  status,
  onClick,
}: {
  src: string;
  label: string;
  selected: boolean;
  status?: OutputInfo['status'];
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-pressed={selected}
      onClick={onClick}
      className={cx(
        'relative h-[72px] w-[72px] shrink-0 overflow-hidden rounded border-2 bg-canvas',
        selected ? 'border-accent' : 'border-transparent hover:border-edge',
      )}
    >
      <img src={src} alt={label} draggable={false} className="h-full w-full object-cover" />
      <span className="absolute inset-x-0 bottom-0 truncate bg-black/65 px-1 text-[10px] leading-4">{label}</span>
      {status === 'approved' && (
        <span className="absolute top-0.5 right-0.5 rounded-full bg-good p-0.5 text-white">
          <Icon name="check" size={10} />
        </span>
      )}
      {status === 'rejected' && (
        <span className="absolute top-0.5 right-0.5 rounded-full bg-bad p-0.5 text-white">
          <Icon name="cross" size={10} />
        </span>
      )}
    </button>
  );
}

export function Filmstrip() {
  const { activeItem, viewedOutput, viewOutput, busy } = useStudio();
  if (!activeItem) return null;
  const progress = busy[activeItem.id];
  const waiting = progress ? Math.max(0, progress.total - progress.done) : 0;

  return (
    <div className="flex h-[92px] shrink-0 items-center gap-2 overflow-x-auto border-t border-line bg-panel px-2">
      <Thumb
        src={api.fileSrc(activeItem.reference)}
        label="Reference"
        selected={!viewedOutput}
        onClick={() => viewOutput(activeItem.id, null)}
      />
      <div className="h-14 w-px shrink-0 bg-edge" />
      {activeItem.outputs.map((output) => (
        <Thumb
          key={output.id}
          src={api.fileSrc(output.path)}
          label={output.styleName}
          status={output.status}
          selected={viewedOutput?.id === output.id}
          onClick={() => viewOutput(activeItem.id, output.id)}
        />
      ))}
      {Array.from({ length: waiting }, (_, index) => (
        <div
          key={index}
          className="flex h-[72px] w-[72px] shrink-0 items-center justify-center rounded border border-dashed border-edge"
        >
          <Spinner />
        </div>
      ))}
      {activeItem.outputs.length === 0 && waiting === 0 && (
        <p className="px-2 text-dim">Generated images will appear here.</p>
      )}
    </div>
  );
}
