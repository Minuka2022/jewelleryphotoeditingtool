// The right-hand column (Properties, Styles, History) and the filmstrip of
// generated images under the work area.

import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { api } from '../api';
import { formatUsd, styleFits, useStudio } from '../store';
import { MAX_REFERENCES } from '../types';
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

/** Every photo of the piece. The first is the main one, whose angle results follow. */
function ReferencePhotos({ item }: { item: ItemInfo }) {
  const { addReferencePhotos, removeReference, makePrimaryReference, busy } = useStudio();
  const full = item.references.length >= MAX_REFERENCES;
  const locked = Boolean(busy[item.id]);

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <span className="text-[11px] text-dim" data-reference-count>
          Photos of this piece ({item.references.length}/{MAX_REFERENCES})
        </span>
        <button
          type="button"
          disabled={full || locked}
          title={full ? `A piece can have at most ${MAX_REFERENCES} photos` : 'Add photos of the same piece from other angles'}
          onClick={() => addReferencePhotos(item)}
          className="flex items-center gap-1 text-[11px] text-accent hover:brightness-125 disabled:opacity-40"
        >
          <Icon name="plus" size={11} /> Add angle
        </button>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {item.references.map((reference, index) => {
          const primary = index === 0;
          return (
            <div key={reference.id} className="group relative" data-reference={primary ? 'main' : 'extra'}>
              <img
                src={api.fileSrc(reference.path)}
                alt={primary ? 'Main photo' : `Photo ${index + 1}`}
                title={primary ? 'Main photo: results follow its viewing angle' : 'Extra view: helps the AI understand the design'}
                className={cx(
                  'h-12 w-12 rounded border-2 bg-canvas object-cover',
                  primary ? 'border-accent' : 'border-edge',
                )}
              />
              {primary && (
                <span className="pointer-events-none absolute inset-x-0 bottom-0 rounded-b bg-accent/90 text-center text-[9px] leading-3 font-semibold text-white">
                  MAIN
                </span>
              )}
              {!primary && !locked && (
                <div className="absolute inset-0 hidden flex-col items-center justify-center gap-0.5 rounded bg-black/70 group-focus-within:flex group-hover:flex">
                  <button
                    type="button"
                    aria-label={`Make photo ${index + 1} the main photo`}
                    title="Make this the main photo"
                    onClick={() => makePrimaryReference(item, reference.id)}
                    className="rounded px-1 text-[9px] font-semibold text-white hover:bg-accent"
                  >
                    MAIN
                  </button>
                  <button
                    type="button"
                    aria-label={`Remove photo ${index + 1}`}
                    title="Remove this photo"
                    onClick={() => removeReference(item, reference.id)}
                    className="rounded p-0.5 text-ink hover:text-bad"
                  >
                    <Icon name="trash" size={11} />
                  </button>
                </div>
              )}
            </div>
          );
        })}
      </div>
      {item.references.length === 1 && (
        <p className="mt-1 text-[11px] leading-snug text-dim">
          More angles (side, top, back) help the AI copy the design exactly.
        </p>
      )}
    </div>
  );
}

/** What the AI found when it studied the photos, editable. */
function DesignNotes({ item }: { item: ItemInfo }) {
  const { saveDesignNotes, studyPhotos, busy } = useStudio();
  const [text, setText] = useState(item.designNotes ?? '');
  useEffect(() => setText(item.designNotes ?? ''), [item.designNotes]);
  const progress = busy[item.id];
  const studying = progress?.phase === 'studying';
  const photos = item.references.length;

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <span className="text-[11px] text-dim">Design notes</span>
        <button
          type="button"
          disabled={Boolean(progress)}
          onClick={() => studyPhotos(item)}
          title={`The AI studies all ${photos} photo(s) and describes the design. A text answer costs a small fraction of one image.`}
          className="flex items-center gap-1 text-[11px] text-accent hover:brightness-125 disabled:opacity-40"
        >
          {studying ? <Spinner size={10} /> : <Icon name="zoom" size={11} />}
          {studying ? 'Studying...' : item.designNotes ? 'Study again' : 'Study photos'}
        </button>
      </div>
      <textarea
        aria-label="Design notes"
        className={cx(inputClass, 'h-24 resize-y text-[11px] leading-snug')}
        placeholder={`Press "Study photos" and the AI describes this piece from all ${photos} photo(s): metal, stones, prongs, engravings. This happens by itself before the first generation. You can correct it.`}
        value={text}
        disabled={studying}
        onChange={(event) => setText(event.target.value)}
        onBlur={() => text.trim() !== (item.designNotes ?? '') && saveDesignNotes(item, text)}
      />
      <p className="text-[11px] leading-snug text-dim">Sent with every generation of this piece.</p>
    </div>
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
    <div className="max-h-[52vh] space-y-2.5 overflow-y-auto p-3">
      <div className="flex gap-2">
        <input
          aria-label="Photo name"
          className={cx(inputClass, 'flex-1')}
          value={name}
          onChange={(event) => setName(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => event.key === 'Enter' && event.currentTarget.blur()}
        />
        <Select
          aria-label="Jewelry type"
          className="h-[26px] w-28"
          value={item.category}
          onChange={(event) => updateItem(item.id, { category: event.target.value })}
        >
          {library?.categories.map((category) => (
            <option key={category.id} value={category.id}>
              {category.label}
            </option>
          ))}
        </Select>
      </div>
      <ReferencePhotos item={item} />
      <DesignNotes key={item.id} item={item} />
      <p className="text-[11px] text-dim">
        Main photo {item.width} x {item.height} px, {item.outputs.length} generated
      </p>
    </div>
  );
}

const swatches: Record<Tone, string> = {
  bright: 'linear-gradient(135deg, #f4f2ee, #c9c4bb)',
  neutral: 'linear-gradient(135deg, #9a968f, #5e5b56)',
  dark: 'linear-gradient(135deg, #3d3d44, #0c0c0e)',
};

function StyleCard({ style, item }: { style: StyleInfo; item: ItemInfo | null }) {
  const { library, selectedStyles, toggleStyle, removeStyle, openStyleDialog } = useStudio();
  const selected = selectedStyles.includes(style.id);
  const fits = !item || styleFits(style, item.category);
  const labelOf = (id?: string) => library?.categories.find((category) => category.id === id)?.label;
  const categoryLabel = labelOf(item?.category);
  // The jewelry types this style is filed under.
  const filedUnder =
    style.categories.length === 0 ? 'All types' : style.categories.map((id) => labelOf(id) ?? id).join(', ');

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
        {style.look === 'render' && (
          <span className="absolute top-1 left-1 rounded bg-black/70 px-1 text-[10px] font-semibold text-ink">3D</span>
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
        <span className="block truncate px-1.5 pb-1 text-[10px] text-dim" title={filedUnder}>
          {filedUnder}
        </span>
      </button>
      <div className="absolute top-1 right-1 hidden gap-1 group-focus-within:flex group-hover:flex">
        <button
          type="button"
          aria-label={`Edit the style ${style.name}`}
          title="Edit this style"
          onClick={() => openStyleDialog(style)}
          className="rounded bg-black/70 p-1 text-ink hover:text-accent"
        >
          <Icon name="edit" size={11} />
        </button>
        <button
          type="button"
          aria-label={`Delete the style ${style.name}`}
          title="Delete this style"
          onClick={() => removeStyle(style)}
          className="rounded bg-black/70 p-1 text-ink hover:text-bad"
        >
          <Icon name="trash" size={11} />
        </button>
      </div>
    </div>
  );
}

/** The Styles panel: a filter by jewelry type above the grid of styles. */
function Styles({ item }: { item: ItemInfo | null }) {
  const { library, styleFilter, setStyleFilter } = useStudio();
  if (!library) return null;

  const types = library.categories.filter((category) => category.id !== 'jewelry');
  const itemLabel = library.categories.find((category) => category.id === item?.category)?.label;
  // "auto" means the open photo's type; with no photo open it shows everything.
  const filter = styleFilter === 'auto' && !item ? 'all' : styleFilter;
  const wanted = filter === 'auto' ? item!.category : filter;
  const visible = filter === 'all' ? library.styles : library.styles.filter((style) => styleFits(style, wanted));
  const wantedLabel = library.categories.find((category) => category.id === wanted)?.label ?? 'this type';

  return (
    <>
      <div className="flex shrink-0 items-center gap-2 border-b border-line px-2 py-1.5">
        <span className="text-[11px] text-dim">Show</span>
        <Select
          aria-label="Filter styles by jewelry type"
          className="min-w-0 flex-1"
          value={filter}
          onChange={(event) => setStyleFilter(event.target.value)}
        >
          {item && <option value="auto">For this photo ({itemLabel})</option>}
          <option value="all">All styles</option>
          {types.map((category) => (
            <option key={category.id} value={category.id}>
              {category.label}
            </option>
          ))}
        </Select>
        <span className="text-[11px] text-dim tabular-nums">{visible.length}</span>
      </div>
      <div className="grid grid-cols-2 content-start gap-2 overflow-y-auto p-2">
        {visible.map((style) => (
          <StyleCard key={style.id} style={style} item={item} />
        ))}
        {visible.length === 0 && (
          <p className="col-span-2 px-1 py-2 text-dim">
            {library.styles.length === 0
              ? 'You have no styles yet. Use Import style to add one.'
              : `No styles for ${wantedLabel} yet. Import one, or choose All styles.`}
          </p>
        )}
      </div>
    </>
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
  const { activeItem, openStyleDialog } = useStudio();
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
            onClick={() => openStyleDialog()}
          >
            <Icon name="plus" size={11} /> Import style
          </button>
        }
      >
        <Styles item={activeItem} />
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
