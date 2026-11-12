// The one place the interface talks to the backend. The same interface runs
// in three places:
//   - the desktop app (Tauri), which calls the Rust backend directly;
//   - the web version, a local server that serves this page and the same
//     operations over HTTP;
//   - a plain browser preview with a stand-in backend (mock.ts).

import { getVersion } from '@tauri-apps/api/app';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import { mockApi } from './mock';
import type {
  AddItemsResult,
  AddReferencesResult,
  Crop,
  ExportEstimate,
  ExportOptions,
  GenerateArgs,
  ImportStyle,
  ImportedStyle,
  ItemInfo,
  LibraryInfo,
  OutputRef,
  OutputStatus,
  Progress,
  PromptCheck,
  Settings,
  SettingsInfo,
  UpdateInfo,
} from './types';

/** How many images were exported and where they went, in words for the user. */
export interface ExportResult {
  count: number;
  where: string;
}

export interface Api {
  getLibrary(): Promise<LibraryInfo>;
  getSettings(): Promise<SettingsInfo>;
  saveSettings(settings: Settings): Promise<SettingsInfo>;
  /** A blank key removes the saved key. */
  setApiKey(key: string): Promise<SettingsInfo>;
  checkApiKey(): Promise<string>;
  listItems(): Promise<ItemInfo[]>;
  addItems(paths: string[], category: string): Promise<AddItemsResult>;
  updateItem(id: string, patch: { name?: string; category?: string }): Promise<ItemInfo>;
  removeItem(id: string): Promise<void>;
  /** More photos of the same piece. */
  addReferences(itemId: string, paths: string[]): Promise<AddReferencesResult>;
  removeReference(itemId: string, referenceId: string): Promise<ItemInfo>;
  makePrimaryReference(itemId: string, referenceId: string): Promise<ItemInfo>;
  /** Blank text removes the notes. */
  setDesignNotes(itemId: string, notes: string | null): Promise<ItemInfo>;
  /** Has the AI study every photo of the piece and write its design notes. */
  analyzeItem(itemId: string): Promise<ItemInfo>;
  setOutputStatus(itemId: string, outputId: string, status: OutputStatus): Promise<ItemInfo>;
  removeOutput(itemId: string, outputId: string): Promise<ItemInfo>;
  /** null removes the crop. */
  setOutputCrop(itemId: string, outputId: string, crop: Crop | null): Promise<ItemInfo>;
  estimateExport(selection: OutputRef, options: ExportOptions): Promise<ExportEstimate>;
  /** Exports to a folder (desktop) or as a download (web); null if cancelled. */
  exportImages(selections: OutputRef[], options: ExportOptions): Promise<ExportResult | null>;
  checkStylePrompt(prompt: string): Promise<PromptCheck>;
  importStyle(style: ImportStyle): Promise<ImportedStyle>;
  /** A null `imagePath` keeps the style's current picture. */
  updateStyle(id: string, style: ImportStyle): Promise<ImportedStyle>;
  removeStyle(id: string): Promise<LibraryInfo>;
  generate(args: GenerateArgs): Promise<ItemInfo>;
  getVersion(): Promise<string>;
  /** Resolves to null when this is the newest version. */
  checkForUpdate(): Promise<UpdateInfo | null>;
  /** Downloads and installs the newer version, then restarts the app. */
  installUpdate(): Promise<void>;
  /** Both return a function that stops listening. */
  onProgress(handler: (progress: Progress) => void): Promise<() => void>;
  onFilesDropped(handler: (paths: string[]) => void, onError: (message: string) => void): Promise<() => void>;
  /** Paths the backend can read; empty or null if the user cancelled. */
  pickPhotos(): Promise<string[]>;
  pickImage(): Promise<string | null>;
  /** Turns a file path from the backend into something an <img> can show. */
  fileSrc(path: string): string;
}

type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** The operations both real backends offer under the same names. */
function operations(call: Call) {
  return {
    getLibrary: () => call<LibraryInfo>('get_library'),
    getSettings: () => call<SettingsInfo>('get_settings'),
    saveSettings: (settings: Settings) => call<SettingsInfo>('save_settings', { settings }),
    setApiKey: (key: string) => call<SettingsInfo>('set_api_key', { key }),
    checkApiKey: () => call<string>('check_api_key'),
    listItems: () => call<ItemInfo[]>('list_items'),
    addItems: (paths: string[], category: string) => call<AddItemsResult>('add_items', { paths, category }),
    updateItem: (id: string, patch: { name?: string; category?: string }) =>
      call<ItemInfo>('update_item', { id, ...patch }),
    removeItem: (id: string) => call<void>('remove_item', { id }),
    addReferences: (itemId: string, paths: string[]) => call<AddReferencesResult>('add_references', { itemId, paths }),
    removeReference: (itemId: string, referenceId: string) =>
      call<ItemInfo>('remove_reference', { itemId, referenceId }),
    makePrimaryReference: (itemId: string, referenceId: string) =>
      call<ItemInfo>('make_primary_reference', { itemId, referenceId }),
    setDesignNotes: (itemId: string, notes: string | null) => call<ItemInfo>('set_design_notes', { itemId, notes }),
    analyzeItem: (itemId: string) => call<ItemInfo>('analyze_item', { itemId }),
    setOutputStatus: (itemId: string, outputId: string, status: OutputStatus) =>
      call<ItemInfo>('set_output_status', { itemId, outputId, status }),
    removeOutput: (itemId: string, outputId: string) => call<ItemInfo>('remove_output', { itemId, outputId }),
    setOutputCrop: (itemId: string, outputId: string, crop: Crop | null) =>
      call<ItemInfo>('set_output_crop', { itemId, outputId, crop }),
    estimateExport: (selection: OutputRef, options: ExportOptions) =>
      call<ExportEstimate>('estimate_export', { selection, options }),
    checkStylePrompt: (prompt: string) => call<PromptCheck>('check_style_prompt', { prompt }),
    importStyle: (style: ImportStyle) => call<ImportedStyle>('import_style', { style }),
    updateStyle: (id: string, style: ImportStyle) => call<ImportedStyle>('update_style', { id, style }),
    removeStyle: (id: string) => call<LibraryInfo>('remove_style', { id }),
    generate: (args: GenerateArgs) => call<ItemInfo>('generate', { args }),
  };
}

// --- Desktop app -----------------------------------------------------------

const PHOTO_FILTER = [{ name: 'Photos', extensions: ['jpg', 'jpeg', 'png', 'webp'] }];

const tauriApi: Api = {
  ...operations((command, args) => invoke(command, args)),
  exportImages: async (selections, options) => {
    const destination = await open({ directory: true });
    if (!destination) return null;
    const count = await invoke<number>('export_images', { selections, options, destination });
    return { count, where: destination };
  },
  getVersion,
  checkForUpdate: () => invoke('check_for_update'),
  installUpdate: () => invoke('install_update'),
  onProgress: (handler) => listen<Progress>('generation-progress', (event) => handler(event.payload)),
  onFilesDropped: (handler) =>
    getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'drop') handler(event.payload.paths);
    }),
  pickPhotos: async () => (await open({ multiple: true, filters: PHOTO_FILTER })) ?? [],
  pickImage: () => open({ multiple: false, filters: PHOTO_FILTER }),
  fileSrc: (path) => convertFileSrc(path),
};

// --- Web version -------------------------------------------------------------

/** Sent with every request so the server knows it comes from this page. */
const HEADERS = { 'x-jewelry-studio': '1' };

async function failure(response: Response): Promise<string> {
  const body = await response.json().catch(() => null);
  return typeof body === 'string' ? body : `The request failed (${response.status}).`;
}

const webCall: Call = async (command, args = {}) => {
  const response = await fetch(`/api/invoke/${command}`, {
    method: 'POST',
    headers: { ...HEADERS, 'content-type': 'application/json' },
    body: JSON.stringify(args),
  });
  if (!response.ok) throw await failure(response);
  return response.json();
};

/** Sends a file to the server and returns where the server saved it. */
async function upload(file: File): Promise<string> {
  const response = await fetch(`/api/upload?name=${encodeURIComponent(file.name)}`, {
    method: 'POST',
    headers: { ...HEADERS, 'content-type': 'application/octet-stream' },
    body: file,
  });
  if (!response.ok) throw await failure(response);
  return response.json();
}

/** Opens the browser's file picker. Resolves to nothing if cancelled. */
function chooseFiles(multiple: boolean): Promise<File[]> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = multiple;
    input.accept = '.jpg,.jpeg,.png,.webp,image/jpeg,image/png,image/webp';
    input.addEventListener('change', () => resolve([...(input.files ?? [])]));
    input.addEventListener('cancel', () => resolve([]));
    input.click();
  });
}

function download(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

const webApi: Api = {
  ...operations(webCall),
  exportImages: async (selections, options) => {
    const response = await fetch('/api/export', {
      method: 'POST',
      headers: { ...HEADERS, 'content-type': 'application/json' },
      body: JSON.stringify({ selections, options }),
    });
    if (!response.ok) throw await failure(response);
    const name = decodeURIComponent(response.headers.get('x-export-name') ?? 'jewelry-studio-export');
    const blob = await response.blob();
    if (blob.size === 0) throw 'The export came back empty. Please try again.';
    download(blob, name);
    const count = Number(response.headers.get('x-export-count') ?? selections.length);
    return { count, where: `your browser's downloads (${name})` };
  },
  getVersion: () => webCall('get_version'),
  checkForUpdate: async () => null,
  installUpdate: async () => {
    throw 'The web version updates when you start the newest copy of the program.';
  },
  onProgress: async (handler) => {
    const events = new EventSource('/api/events');
    events.addEventListener('generation-progress', (event) => handler(JSON.parse((event as MessageEvent).data)));
    return () => events.close();
  },
  onFilesDropped: async (handler, onError) => {
    const allow = (event: DragEvent) => event.preventDefault();
    const drop = async (event: DragEvent) => {
      event.preventDefault();
      const files = [...(event.dataTransfer?.files ?? [])];
      if (files.length === 0) return;
      try {
        handler(await Promise.all(files.map(upload)));
      } catch (error) {
        onError(errorText(error));
      }
    };
    window.addEventListener('dragover', allow);
    window.addEventListener('drop', drop);
    return () => {
      window.removeEventListener('dragover', allow);
      window.removeEventListener('drop', drop);
    };
  },
  pickPhotos: async () => Promise.all((await chooseFiles(true)).map(upload)),
  pickImage: async () => {
    const [file] = await chooseFiles(false);
    return file ? upload(file) : null;
  },
  fileSrc: (path) => `/api/file?path=${encodeURIComponent(path)}`,
};

// --- Which one ----------------------------------------------------------------

export const inDesktopApp = '__TAURI_INTERNALS__' in window;

/** The web version's server marks the page it serves. */
export const inWebVersion =
  document.querySelector('meta[name="jewelry-studio"]')?.getAttribute('content') === 'web';

export const api: Api = inDesktopApp ? tauriApi : inWebVersion ? webApi : mockApi;

/** The backend reports problems as plain sentences meant for the user. */
export function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return 'Something went wrong.';
}
