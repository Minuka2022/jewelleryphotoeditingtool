// A stand-in backend used only when the interface is opened in a normal
// browser instead of the desktop app. It keeps everything in memory and draws
// placeholder pictures, so the interface can be developed and checked without
// spending money on real generations.

import type { Api } from './api';
import type { Crop, ItemInfo, LibraryInfo, OutputInfo, Progress, SettingsInfo, StyleInfo } from './types';

const picture = (background: string, label: string) =>
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="900" viewBox="0 0 1200 900">
      <rect width="1200" height="900" fill="${background}"/>
      <ellipse cx="600" cy="540" rx="210" ry="180" fill="none" stroke="#e2b85c" stroke-width="34"/>
      <path d="M600 250l72 72-72 84-72-84z" fill="#9bd7ff" stroke="#fff" stroke-width="6"/>
      <text x="40" y="860" font-family="Segoe UI, sans-serif" font-size="32" fill="#8a8a8a">${label}</text>
    </svg>`,
  );

const backgrounds: Record<string, string> = {
  'black-velvet': '#131316',
  'dark-slate': '#3a3e45',
  'studio-white': '#f4f4f1',
  'white-marble': '#e9e7e2',
};

const style = (id: string, name: string, tone: StyleInfo['tone'], description: string): StyleInfo => ({
  id,
  name,
  version: 1,
  description,
  tone,
  look: 'photo',
  categories: [],
  prompt: `Place the {item} in the ${name} scene.`,
  thumbnail: null,
});

let library: LibraryInfo = {
  categories: [
    { id: 'jewelry', label: 'Jewelry (general)' },
    { id: 'ring', label: 'Ring' },
    { id: 'earrings', label: 'Earrings' },
    { id: 'pendant', label: 'Pendant' },
    { id: 'necklace', label: 'Necklace' },
    { id: 'bracelet', label: 'Bracelet' },
  ],
  styles: [
    style('black-velvet', 'Black Velvet', 'dark', 'Dark, luxurious boutique look with a single spotlight.'),
    style('dark-slate', 'Dark Slate', 'dark', 'Moody, modern look on textured slate.'),
    style('studio-white', 'Studio White', 'bright', 'Clean white catalogue look with a soft shadow.'),
    style('white-marble', 'White Marble', 'bright', 'Bright, airy lifestyle look on marble.'),
  ],
  models: [
    { id: 'gemini-nano-banana-2.1', label: 'Nano Banana 2.1', note: 'Newest and cheapest.', prices: { '1K': 0.0336, '2K': 0.0504, '4K': 0.113 } },
    { id: 'gemini-3-pro-image', label: 'Nano Banana Pro', note: 'Highest quality.', prices: { '1K': 0.134, '2K': 0.134, '4K': 0.24 } },
  ],
  pricesCheckedOn: '2026-11-12',
  problems: [],
};

let settings: SettingsInfo = {
  model: 'gemini-nano-banana-2.1',
  imageSize: '2K',
  aspectRatio: null,
  variants: 2,
  hasApiKey: true,
};

let nextId = 1;
const id = (prefix: string) => `${prefix}${nextId++}`;
const now = () => new Date().toISOString();
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const copy = <T,>(value: T): T => JSON.parse(JSON.stringify(value));

const output = (itemName: string, styleId: string, runId: string, number: number): OutputInfo => {
  const info = library.styles.find((s) => s.id === styleId)!;
  return {
    id: id('o'),
    runId,
    styleId,
    styleName: info.name,
    styleVersion: info.version,
    status: 'none',
    createdAt: now(),
    model: settings.model,
    imageSize: settings.imageSize,
    costUsd: 0.0504,
    note: null,
    ...(() => {
      const path = picture(backgrounds[styleId] ?? '#6d5a7a', `${itemName} / ${info.name} / option ${number}`);
      return { path, originalPath: path };
    })(),
  };
};

/** The placeholder picture cut down to the crop, like the real cropped copy. */
const croppedPicture = (original: string, crop: Crop) => {
  const svg = decodeURIComponent(original.slice('data:image/svg+xml;utf8,'.length));
  const [x, y, w, h] = [crop.x * 1200, crop.y * 900, crop.width * 1200, crop.height * 900].map(Math.round);
  const cut = svg.replace(
    'width="1200" height="900" viewBox="0 0 1200 900"',
    `width="${w}" height="${h}" viewBox="${x} ${y} ${w} ${h}"`,
  );
  return 'data:image/svg+xml;utf8,' + encodeURIComponent(cut);
};

const item = (name: string, category: string): ItemInfo => {
  const reference = picture('#96897a', `${name} (reference)`);
  return {
    id: id('i'),
    name,
    category,
    width: 1200,
    height: 900,
    createdAt: now(),
    reference,
    references: [{ id: 'primary', path: reference, width: 1200, height: 900 }],
    designNotes: null,
    outputs: [],
  };
};

const syncPrimary = (found: ItemInfo) => {
  found.reference = found.references[0].path;
};

const items: ItemInfo[] = [];
// "?demo" opens the preview with a photo and results already in place.
if (new URLSearchParams(location.search).has('demo')) {
  const ring = item('solitaire-01', 'ring');
  ring.outputs = [
    output(ring.name, 'black-velvet', 'r0', 1),
    { ...output(ring.name, 'black-velvet', 'r0', 2), status: 'approved' },
    output(ring.name, 'studio-white', 'r0', 1),
    { ...output(ring.name, 'studio-white', 'r0', 2), status: 'rejected' },
  ];
  items.push(ring, item('pearl-studs', 'earrings'));
}

const find = (itemId: string) => {
  const found = items.find((i) => i.id === itemId);
  if (!found) throw 'That photo is no longer in the workspace.';
  return found;
};

const progressHandlers = new Set<(progress: Progress) => void>();

export const mockApi: Api = {
  getLibrary: async () => copy(library),
  getSettings: async () => copy(settings),
  saveSettings: async (next) => copy((settings = { ...settings, ...next })),
  setApiKey: async (key) => copy((settings = { ...settings, hasApiKey: key.trim() !== '' })),
  checkApiKey: async () => `The key works and ${settings.model} is available.`,
  listItems: async () => copy(items),
  addItems: async (paths, category) => {
    const added = paths.map((path) => item(path.replace(/^.*[\\/]/, '').replace(/\.\w+$/, ''), category));
    items.push(...added);
    return { items: copy(added), errors: [] };
  },
  updateItem: async (itemId, patch) => copy(Object.assign(find(itemId), patch)),
  removeItem: async (itemId) => void items.splice(items.indexOf(find(itemId)), 1),
  addReferences: async (itemId, paths) => {
    const found = find(itemId);
    const room = 6 - found.references.length;
    const tints = ['#8a7d6f', '#7d8a86', '#857a8c', '#8c847a', '#7a828c'];
    paths.slice(0, room).forEach((path, index) => {
      const angle = path.replace(/^.*[\\/]/, '');
      found.references.push({ id: id('f'), path: picture(tints[index % tints.length], `${found.name} (${angle})`), width: 1200, height: 900 });
    });
    const errors = paths.length > room ? ['A piece can have at most 6 photos. Remove one to add another.'] : [];
    return { item: copy(found), errors };
  },
  removeReference: async (itemId, referenceId) => {
    const found = find(itemId);
    if (found.references.length <= 1) throw 'A piece needs at least one photo.';
    found.references = found.references.filter((r) => r.id !== referenceId);
    syncPrimary(found);
    return copy(found);
  },
  makePrimaryReference: async (itemId, referenceId) => {
    const found = find(itemId);
    const chosen = found.references.find((r) => r.id === referenceId);
    if (!chosen) throw 'That photo is no longer part of this piece.';
    found.references = [chosen, ...found.references.filter((r) => r.id !== referenceId)];
    syncPrimary(found);
    return copy(found);
  },
  setDesignNotes: async (itemId, notes) => {
    const found = find(itemId);
    found.designNotes = notes?.trim() || null;
    return copy(found);
  },
  analyzeItem: async (itemId) => {
    await wait(900);
    const found = find(itemId);
    found.designNotes = [
      `- Studied ${found.references.length} photo(s) of this ${found.category}.`,
      '- Yellow gold band, polished, rounded profile.',
      '- Oval light-blue centre stone in 4 prongs.',
      '- Halo of 12 small round white stones.',
    ].join('\n');
    return copy(found);
  },
  setOutputStatus: async (itemId, outputId, status) => {
    const found = find(itemId);
    found.outputs = found.outputs.map((o) => (o.id === outputId ? { ...o, status } : o));
    return copy(found);
  },
  removeOutput: async (itemId, outputId) => {
    const found = find(itemId);
    found.outputs = found.outputs.filter((o) => o.id !== outputId);
    return copy(found);
  },
  setOutputCrop: async (itemId, outputId, crop) => {
    await wait(120);
    const found = find(itemId);
    found.outputs = found.outputs.map((o) =>
      o.id === outputId ? { ...o, crop, path: crop ? croppedPicture(o.originalPath, crop) : o.originalPath } : o,
    );
    return copy(found);
  },
  estimateExport: async (selection, options) => {
    await wait(150);
    const output = find(selection.itemId).outputs.find((o) => o.id === selection.outputId);
    const crop = output?.crop ?? { x: 0, y: 0, width: 1, height: 1 };
    const sourceWidth = Math.round(1200 * crop.width);
    const sourceHeight = Math.round(900 * crop.height);
    const width = options.width ?? sourceWidth;
    const height = Math.max(1, Math.round((width * sourceHeight) / sourceWidth));
    const perPixel = options.format === 'jpeg' ? 0.15 + (options.quality / 100) ** 3 * 1.6 : 2.2;
    return { width, height, sourceWidth, sourceHeight, bytes: Math.round(width * height * perPixel) };
  },
  exportImages: async (selections) => {
    await wait(400);
    return { count: selections.length, where: 'C:\\Exports' };
  },
  checkStylePrompt: async (prompt) => ({
    warnings: /\b(hand|worn|side view|rotate)\b/i.test(prompt)
      ? ['"hand" shows the item worn or on a body']
      : [],
    itemWords: ['ring', 'gold', 'diamond'].filter((word) => new RegExp(`\\b${word}\\b`, 'i').test(prompt)),
  }),
  importStyle: async (input) => {
    const styleId = input.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
    if (!styleId) throw 'Give the style a name that contains letters or digits.';
    if (library.styles.some((s) => s.id === styleId)) throw `A style named "${input.name}" already exists. Choose another name.`;
    const created: StyleInfo = {
      id: styleId,
      name: input.name.trim(),
      version: 1,
      description: input.description,
      tone: input.tone,
      look: input.look,
      categories: input.categories,
      prompt: input.prompt,
      thumbnail: input.imagePath ? picture('#7a5a6a', input.name) : null,
    };
    library = { ...library, styles: [...library.styles, created] };
    return { styleId, library: copy(library) };
  },
  updateStyle: async (styleId, input) => {
    const current = library.styles.find((s) => s.id === styleId);
    if (!current) throw `There is no style "${styleId}".`;
    if (!input.name.trim()) throw 'Give the style a name.';
    const changesResults = input.prompt !== current.prompt || input.look !== current.look;
    const updated: StyleInfo = {
      ...current,
      name: input.name.trim(),
      description: input.description,
      tone: input.tone,
      look: input.look,
      categories: input.categories,
      prompt: input.prompt,
      version: current.version + (changesResults ? 1 : 0),
      thumbnail: input.imagePath ? picture('#5a6a7a', input.name) : current.thumbnail,
    };
    library = { ...library, styles: library.styles.map((s) => (s.id === styleId ? updated : s)) };
    return { styleId, library: copy(library) };
  },
  removeStyle: async (styleId) => {
    library = { ...library, styles: library.styles.filter((s) => s.id !== styleId) };
    return copy(library);
  },
  generate: async (args) => {
    const found = find(args.itemId);
    const runId = id('r');
    const total = args.styleIds.length * args.variants;
    let done = 0;
    for (const styleId of args.styleIds) {
      for (let number = 1; number <= args.variants; number++) {
        await wait(700);
        const created = output(found.name, styleId, runId, number);
        found.outputs.push(created);
        done += 1;
        const progress: Progress = {
          itemId: found.id,
          runId,
          done,
          total,
          styleId,
          styleName: created.styleName,
          number,
          output: copy(created),
          error: null,
        };
        progressHandlers.forEach((handler) => handler(progress));
      }
    }
    return copy(found);
  },
  getVersion: async () => '0.2.0',
  // "?update" previews the update banner.
  checkForUpdate: async () =>
    new URLSearchParams(location.search).has('update')
      ? { version: '0.3.0', currentVersion: '0.2.0', notes: 'New styles and faster generation.' }
      : null,
  installUpdate: async () => {
    await wait(1500);
    throw 'Updates cannot be installed in the browser preview.';
  },
  onProgress: async (handler) => {
    progressHandlers.add(handler);
    return () => void progressHandlers.delete(handler);
  },
  onFilesDropped: async () => () => {},
  pickPhotos: async () => [`sample-${nextId}.jpg`],
  pickImage: async () => 'example-look.jpg',
  fileSrc: (path) => path,
};
