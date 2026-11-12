// All of the interface's state and the actions that change it.

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { api, errorText } from './api';
import type {
  Crop,
  ImportStyle,
  ItemInfo,
  LibraryInfo,
  OutputInfo,
  OutputRef,
  OutputStatus,
  Settings,
  SettingsInfo,
  StyleInfo,
  UpdateInfo,
} from './types';

export type CompareMode = 'single' | 'split' | 'slider';
export type Tool = 'hand' | 'zoom' | 'crop';
export type DialogName = 'settings' | 'import-style' | 'shortcuts' | 'export';

export const FULL_CROP: Crop = { x: 0, y: 0, width: 1, height: 1 };

/** Crop shapes offered in the crop tool, as width:height. "original" keeps the image's own shape. */
export const CROP_RATIOS: { id: string; label: string; ratio: number | 'original' | null }[] = [
  { id: 'free', label: 'Free', ratio: null },
  { id: 'original', label: 'Original', ratio: 'original' },
  { id: '1:1', label: '1 : 1 (square)', ratio: 1 },
  { id: '4:5', label: '4 : 5', ratio: 4 / 5 },
  { id: '3:4', label: '3 : 4', ratio: 3 / 4 },
  { id: '2:3', label: '2 : 3', ratio: 2 / 3 },
  { id: '4:3', label: '4 : 3', ratio: 4 / 3 },
  { id: '3:2', label: '3 : 2', ratio: 3 / 2 },
  { id: '16:9', label: '16 : 9', ratio: 16 / 9 },
];

/** An unsaved crop being adjusted in the crop tool. */
export interface CropDraft {
  itemId: string;
  outputId: string;
  crop: Crop;
  ratioId: string;
  /** The image's full size in pixels, once it has loaded. */
  natural: { width: number; height: number } | null;
}

/** The largest crop of the given pixel ratio that fits the image, centred. */
export function centredCrop(ratio: number, natural: { width: number; height: number }): Crop {
  // In fractions, a pixel ratio r becomes r * height / width.
  const fractionRatio = (ratio * natural.height) / natural.width;
  const width = Math.min(1, fractionRatio);
  const height = width / fractionRatio;
  return { x: (1 - width) / 2, y: (1 - height) / 2, width, height };
}

/** Zoom 1 means "fit the window"; x and y are the pan offset in pixels. */
export interface View {
  zoom: number;
  x: number;
  y: number;
}

export interface Toast {
  id: number;
  kind: 'info' | 'error';
  text: string;
}

export interface Confirmation {
  title: string;
  body: string;
  action: string;
  danger?: boolean;
  run: () => void;
}

export const FIT: View = { zoom: 1, x: 0, y: 0 };

/** What a photo is busy with: the AI studying its photos, or generating. */
export interface Busy {
  done: number;
  total: number;
  phase: 'studying' | 'generating';
}

/** Batches that cost at least this much ask for confirmation first. */
const CONFIRM_ABOVE_USD = 1;

export function formatUsd(amount: number): string {
  return amount < 10 ? `$${amount.toFixed(2)}` : `$${amount.toFixed(0)}`;
}

export function styleFits(style: StyleInfo, category: string): boolean {
  return style.categories.length === 0 || style.categories.includes(category);
}

function useController() {
  const [library, setLibrary] = useState<LibraryInfo | null>(null);
  const [settings, setSettings] = useState<SettingsInfo | null>(null);
  const [items, setItems] = useState<ItemInfo[]>([]);
  const [startupError, setStartupError] = useState<string | null>(null);

  const [activeId, setActiveId] = useState<string | null>(null);
  // Per photo: the generated image being viewed, or null for the reference.
  const [viewing, setViewing] = useState<Record<string, string | null>>({});
  const [selectedStyles, setSelectedStyles] = useState<string[]>([]);
  // Which styles the panel lists: "auto" follows the open photo's type,
  // "all" lists everything, anything else is a jewelry type id.
  const [styleFilter, setStyleFilter] = useState('auto');
  // The style being edited in the style window, or null when adding a new one.
  const [editingStyle, setEditingStyle] = useState<StyleInfo | null>(null);
  const [note, setNote] = useState('');
  const [lastCategory, setLastCategory] = useState('jewelry');

  const [compare, setCompare] = useState<CompareMode>('split');
  const [tool, setToolState] = useState<Tool>('hand');
  const [cropDraft, setCropDraft] = useState<CropDraft | null>(null);
  const [view, setView] = useState<View>(FIT);
  const [zoomPercent, setZoomPercent] = useState(100);

  // Per photo that is generating: how many images are done out of how many.
  const [busy, setBusy] = useState<Record<string, Busy>>({});
  const [dialog, setDialog] = useState<DialogName | null>(null);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastId = useRef(0);
  const runErrors = useRef<Record<string, string[]>>({});

  const [version, setVersion] = useState('');
  // A newer version that is ready to install, if there is one.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updating, setUpdating] = useState(false);

  const dismissToast = useCallback((id: number) => {
    setToasts((list) => list.filter((toast) => toast.id !== id));
  }, []);

  const notify = useCallback(
    (kind: Toast['kind'], text: string) => {
      const id = ++toastId.current;
      setToasts((list) => [...list.slice(-3), { id, kind, text }]);
      setTimeout(() => dismissToast(id), kind === 'error' ? 15000 : 5000);
    },
    [dismissToast],
  );

  const fail = useCallback((error: unknown) => notify('error', errorText(error)), [notify]);

  const putItem = useCallback((item: ItemInfo) => {
    setItems((list) => list.map((existing) => (existing.id === item.id ? item : existing)));
  }, []);

  // Load everything once, and listen for images arriving from a generation.
  useEffect(() => {
    Promise.all([api.getLibrary(), api.getSettings(), api.listItems()])
      .then(([loadedLibrary, loadedSettings, loadedItems]) => {
        setLibrary(loadedLibrary);
        setSettings(loadedSettings);
        setItems(loadedItems);
        setActiveId(loadedItems[0]?.id ?? null);
        loadedLibrary.problems.forEach((problem) => notify('error', `A style was skipped. ${problem}`));
      })
      .catch((error) => setStartupError(errorText(error)));

    api.getVersion().then(setVersion, () => {});
    // A failed check at startup (no internet, for example) is not worth a message.
    api.checkForUpdate().then(setUpdate, () => {});

    const stopListening = api.onProgress((progress) => {
      setBusy((current) =>
        current[progress.itemId]
          ? { ...current, [progress.itemId]: { done: progress.done, total: progress.total, phase: 'generating' } }
          : current,
      );
      if (progress.error) {
        (runErrors.current[progress.itemId] ??= []).push(progress.error);
      }
      const output = progress.output;
      if (!output) return;
      setItems((list) =>
        list.map((item) =>
          item.id === progress.itemId && !item.outputs.some((o) => o.id === output.id)
            ? { ...item, outputs: [...item.outputs, output] }
            : item,
        ),
      );
      // Show the first result as soon as it arrives.
      setViewing((current) => (current[progress.itemId] ? current : { ...current, [progress.itemId]: output.id }));
    });
    return () => void stopListening.then((stop) => stop());
  }, [notify]);

  const activeItem = useMemo(() => items.find((item) => item.id === activeId) ?? null, [items, activeId]);
  const viewedOutput: OutputInfo | null = useMemo(() => {
    if (!activeItem) return null;
    return activeItem.outputs.find((output) => output.id === viewing[activeItem.id]) ?? null;
  }, [activeItem, viewing]);

  // The selected styles that can be used for the active photo's category.
  const usableStyles = useMemo(() => {
    if (!library || !activeItem) return [];
    return library.styles.filter(
      (style) => selectedStyles.includes(style.id) && styleFits(style, activeItem.category),
    );
  }, [library, activeItem, selectedStyles]);

  const pricePerImage = useMemo(() => {
    if (!library || !settings) return null;
    return library.models.find((model) => model.id === settings.model)?.prices[settings.imageSize] ?? null;
  }, [library, settings]);

  const plannedImages = usableStyles.length * (settings?.variants ?? 0);
  const plannedCost = pricePerImage === null ? null : pricePerImage * plannedImages;
  const isGenerating = activeItem ? Boolean(busy[activeItem.id]) : false;

  const selectItem = useCallback((id: string) => {
    setActiveId(id);
    setView(FIT);
  }, []);

  const viewOutput = useCallback((itemId: string, outputId: string | null) => {
    setViewing((current) => ({ ...current, [itemId]: outputId }));
  }, []);

  /** Moves to the next or previous image in the filmstrip (the reference comes first). */
  const stepOutput = useCallback(
    (direction: 1 | -1) => {
      if (!activeItem) return;
      const order: (string | null)[] = [null, ...activeItem.outputs.map((output) => output.id)];
      const position = order.indexOf(viewedOutput?.id ?? null);
      const next = order[(position + direction + order.length) % order.length];
      viewOutput(activeItem.id, next);
    },
    [activeItem, viewedOutput, viewOutput],
  );

  const addPhotos = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      try {
        const result = await api.addItems(paths, lastCategory);
        setItems((list) => [...list, ...result.items]);
        if (result.items.length > 0) selectItem(result.items[0].id);
        result.errors.forEach((error) => notify('error', error));
      } catch (error) {
        fail(error);
      }
    },
    [lastCategory, selectItem, notify, fail],
  );

  const openPhotos = useCallback(async () => {
    try {
      await addPhotos(await api.pickPhotos());
    } catch (error) {
      fail(error);
    }
  }, [addPhotos, fail]);

  /** Opens several photos as views of one piece: the first is the main one. */
  const openPiece = useCallback(async () => {
    try {
      const [first, ...others] = await api.pickPhotos();
      if (!first) return;
      const result = await api.addItems([first], lastCategory);
      result.errors.forEach((error) => notify('error', error));
      const created = result.items[0];
      if (!created) return;
      let item = created;
      if (others.length > 0) {
        const more = await api.addReferences(created.id, others);
        more.errors.forEach((error) => notify('error', error));
        item = more.item;
      }
      setItems((list) => [...list, item]);
      selectItem(item.id);
    } catch (error) {
      fail(error);
    }
  }, [lastCategory, selectItem, notify, fail]);

  const notesMayBeStale = useCallback(
    (item: ItemInfo) => {
      if (item.designNotes) {
        notify('info', 'The photos changed. Press "Study photos" to update the design notes.');
      }
    },
    [notify],
  );

  const addReferencePhotos = useCallback(
    async (item: ItemInfo) => {
      try {
        const paths = await api.pickPhotos();
        if (paths.length === 0) return;
        const result = await api.addReferences(item.id, paths);
        putItem(result.item);
        result.errors.forEach((error) => notify('error', error));
        notesMayBeStale(result.item);
      } catch (error) {
        fail(error);
      }
    },
    [putItem, notify, notesMayBeStale, fail],
  );

  const removeReference = useCallback(
    (item: ItemInfo, referenceId: string) => {
      setConfirmation({
        title: 'Remove this photo from the piece?',
        body: 'The copy kept by Jewelry Studio is deleted. Your original file is not affected.',
        action: 'Remove',
        danger: true,
        run: async () => {
          try {
            const updated = await api.removeReference(item.id, referenceId);
            putItem(updated);
            notesMayBeStale(updated);
          } catch (error) {
            fail(error);
          }
        },
      });
    },
    [putItem, notesMayBeStale, fail],
  );

  const makePrimaryReference = useCallback(
    async (item: ItemInfo, referenceId: string) => {
      try {
        putItem(await api.makePrimaryReference(item.id, referenceId));
      } catch (error) {
        fail(error);
      }
    },
    [putItem, fail],
  );

  const saveDesignNotes = useCallback(
    async (item: ItemInfo, notes: string) => {
      try {
        putItem(await api.setDesignNotes(item.id, notes.trim() || null));
      } catch (error) {
        fail(error);
      }
    },
    [putItem, fail],
  );

  /** Has the AI study every photo of a piece and write its design notes. */
  const studyPhotos = useCallback(
    async (item: ItemInfo) => {
      setBusy((current) => ({ ...current, [item.id]: { done: 0, total: 0, phase: 'studying' } }));
      try {
        putItem(await api.analyzeItem(item.id));
      } catch (error) {
        fail(error);
      } finally {
        setBusy(({ [item.id]: _finished, ...rest }) => rest);
      }
    },
    [putItem, fail],
  );

  const updateItem = useCallback(
    async (id: string, patch: { name?: string; category?: string }) => {
      try {
        putItem(await api.updateItem(id, patch));
        if (patch.category) setLastCategory(patch.category);
      } catch (error) {
        fail(error);
      }
    },
    [putItem, fail],
  );

  const removeItem = useCallback(
    (item: ItemInfo) => {
      setConfirmation({
        title: `Remove "${item.name}"?`,
        body:
          item.outputs.length > 0
            ? `This deletes the photo and its ${item.outputs.length} generated image(s) from Jewelry Studio. Files you already exported are not affected.`
            : 'This removes the photo from Jewelry Studio. Your original file is not affected.',
        action: 'Remove',
        danger: true,
        run: async () => {
          try {
            await api.removeItem(item.id);
            const next = items.find((existing) => existing.id !== item.id)?.id ?? null;
            setItems((list) => list.filter((existing) => existing.id !== item.id));
            setActiveId((current) => (current === item.id ? next : current));
          } catch (error) {
            fail(error);
          }
        },
      });
    },
    [items, fail],
  );

  const setStatus = useCallback(
    async (status: OutputStatus) => {
      if (!activeItem || !viewedOutput) return;
      // Pressing the same key again clears the mark.
      const next = viewedOutput.status === status ? 'none' : status;
      try {
        putItem(await api.setOutputStatus(activeItem.id, viewedOutput.id, next));
      } catch (error) {
        fail(error);
      }
    },
    [activeItem, viewedOutput, putItem, fail],
  );

  const deleteViewedOutput = useCallback(async () => {
    if (!activeItem || !viewedOutput) return;
    try {
      putItem(await api.removeOutput(activeItem.id, viewedOutput.id));
      viewOutput(activeItem.id, null);
    } catch (error) {
      fail(error);
    }
  }, [activeItem, viewedOutput, putItem, viewOutput, fail]);

  const toggleStyle = useCallback((id: string) => {
    setSelectedStyles((list) => (list.includes(id) ? list.filter((other) => other !== id) : [...list, id]));
  }, []);

  const changeSettings = useCallback(
    async (patch: Partial<Settings>) => {
      if (!settings) return;
      const { hasApiKey: _unused, ...current } = settings;
      try {
        setSettings(await api.saveSettings({ ...current, ...patch }));
      } catch (error) {
        fail(error);
      }
    },
    [settings, fail],
  );

  const runGeneration = useCallback(async () => {
    if (!activeItem || !settings || usableStyles.length === 0) return;
    const itemId = activeItem.id;
    const total = usableStyles.length * settings.variants;
    runErrors.current[itemId] = [];
    try {
      // The AI studies all the photos first, unless that was already done.
      if (!activeItem.designNotes) {
        setBusy((current) => ({ ...current, [itemId]: { done: 0, total, phase: 'studying' } }));
        try {
          putItem(await api.analyzeItem(itemId));
        } catch (error) {
          notify('error', `The photos could not be studied first, so this run has no design notes. ${errorText(error)}`);
        }
      }
      setBusy((current) => ({ ...current, [itemId]: { done: 0, total, phase: 'generating' } }));
      putItem(
        await api.generate({
          itemId,
          styleIds: usableStyles.map((style) => style.id),
          variants: settings.variants,
          note: note.trim() || null,
        }),
      );
      const errors = runErrors.current[itemId] ?? [];
      if (errors.length > 0) {
        notify('error', `${errors.length} of ${total} images could not be generated. ${errors[0]}`);
      }
    } catch (error) {
      fail(error);
    } finally {
      setBusy(({ [itemId]: _finished, ...rest }) => rest);
    }
  }, [activeItem, settings, usableStyles, note, putItem, notify, fail]);

  const generate = useCallback(() => {
    if (!activeItem || isGenerating) return;
    if (!settings?.hasApiKey) {
      setDialog('settings');
      return;
    }
    if (usableStyles.length === 0) {
      notify('info', 'Choose at least one style in the Styles panel first.');
      return;
    }
    if (plannedCost !== null && plannedCost >= CONFIRM_ABOVE_USD) {
      setConfirmation({
        title: `Generate ${plannedImages} images?`,
        body: `This will cost about ${formatUsd(plannedCost)}.`,
        action: 'Generate',
        run: runGeneration,
      });
      return;
    }
    void runGeneration();
  }, [activeItem, isGenerating, settings, usableStyles, plannedCost, plannedImages, notify, runGeneration]);

  /** Every approved image, across all photos. */
  const approvedOutputs: OutputRef[] = useMemo(
    () =>
      items.flatMap((item) =>
        item.outputs
          .filter((output) => output.status === 'approved')
          .map((output) => ({ itemId: item.id, outputId: output.id })),
      ),
    [items],
  );

  const openExport = useCallback(() => {
    if (!viewedOutput && approvedOutputs.length === 0) {
      notify(
        'info',
        'Choose a generated image in the filmstrip to export it, or approve images (press A) to export them together.',
      );
      return;
    }
    setDialog('export');
  }, [viewedOutput, approvedOutputs, notify]);

  // --- Crop tool -----------------------------------------------------------

  const startCrop = useCallback(() => {
    if (!activeItem || !viewedOutput) {
      notify('info', 'Choose a generated image in the filmstrip first. The reference photo cannot be cropped.');
      return;
    }
    setCropDraft({
      itemId: activeItem.id,
      outputId: viewedOutput.id,
      crop: viewedOutput.crop ?? FULL_CROP,
      ratioId: 'free',
      natural: null,
    });
    setView(FIT);
    setToolState('crop');
  }, [activeItem, viewedOutput, notify]);

  const cancelCrop = useCallback(() => {
    setCropDraft(null);
    setToolState('hand');
  }, []);

  const setTool = useCallback(
    (next: Tool) => {
      if (next === 'crop') startCrop();
      else {
        setCropDraft(null);
        setToolState(next);
      }
    },
    [startCrop],
  );

  const applyCrop = useCallback(async () => {
    if (!cropDraft) return;
    const { itemId, outputId, crop } = cropDraft;
    const full = crop.x < 1e-4 && crop.y < 1e-4 && crop.width > 0.9999 && crop.height > 0.9999;
    try {
      putItem(await api.setOutputCrop(itemId, outputId, full ? null : crop));
      cancelCrop();
    } catch (error) {
      fail(error);
    }
  }, [cropDraft, putItem, cancelCrop, fail]);

  const removeCrop = useCallback(
    async (itemId: string, outputId: string) => {
      try {
        putItem(await api.setOutputCrop(itemId, outputId, null));
      } catch (error) {
        fail(error);
      }
    },
    [putItem, fail],
  );

  // Leaving the image being cropped abandons the unsaved crop.
  useEffect(() => {
    if (cropDraft && (cropDraft.itemId !== activeItem?.id || cropDraft.outputId !== viewedOutput?.id)) {
      cancelCrop();
    }
  }, [cropDraft, activeItem, viewedOutput, cancelCrop]);

  /** Opens the style window: empty to add a style, or filled in to edit one. */
  const openStyleDialog = useCallback((style?: StyleInfo) => {
    setEditingStyle(style ?? null);
    setDialog('import-style');
  }, []);

  /** Saves the style window. Returns a message if the style was refused. */
  const saveStyle = useCallback(
    async (style: ImportStyle): Promise<string | null> => {
      try {
        if (editingStyle) {
          const result = await api.updateStyle(editingStyle.id, style);
          setLibrary(result.library);
          notify('info', `"${style.name.trim()}" was updated.`);
        } else {
          const result = await api.importStyle(style);
          setLibrary(result.library);
          setSelectedStyles((list) => [...list, result.styleId]);
          notify('info', `"${style.name.trim()}" was added to your styles.`);
        }
        return null;
      } catch (error) {
        return errorText(error);
      }
    },
    [editingStyle, notify],
  );

  const removeStyle = useCallback(
    (style: StyleInfo) => {
      setConfirmation({
        title: `Delete the style "${style.name}"?`,
        body: 'Images already generated with it are kept.',
        action: 'Delete',
        danger: true,
        run: async () => {
          try {
            setLibrary(await api.removeStyle(style.id));
            setSelectedStyles((list) => list.filter((id) => id !== style.id));
          } catch (error) {
            fail(error);
          }
        },
      });
    },
    [fail],
  );

  const checkForUpdates = useCallback(async () => {
    try {
      const found = await api.checkForUpdate();
      setUpdate(found);
      if (!found) notify('info', `You have the latest version${version ? ` (${version})` : ''}.`);
    } catch (error) {
      fail(error);
    }
  }, [version, notify, fail]);

  /** On success the app closes, installs and restarts, so this never returns. */
  const installUpdate = useCallback(async () => {
    setUpdating(true);
    try {
      await api.installUpdate();
    } catch (error) {
      fail(error);
      setUpdating(false);
    }
  }, [fail]);

  return {
    version,
    update,
    setUpdate,
    updating,
    checkForUpdates,
    installUpdate,
    library,
    settings,
    setSettings,
    items,
    startupError,
    activeItem,
    viewedOutput,
    selectedStyles,
    usableStyles,
    note,
    setNote,
    compare,
    setCompare,
    tool,
    setTool,
    view,
    setView,
    zoomPercent,
    setZoomPercent,
    busy,
    isGenerating,
    plannedImages,
    plannedCost,
    dialog,
    setDialog,
    confirmation,
    setConfirmation,
    toasts,
    dismissToast,
    notify,
    selectItem,
    viewOutput,
    stepOutput,
    addPhotos,
    openPhotos,
    openPiece,
    addReferencePhotos,
    removeReference,
    makePrimaryReference,
    saveDesignNotes,
    studyPhotos,
    updateItem,
    removeItem,
    setStatus,
    deleteViewedOutput,
    toggleStyle,
    changeSettings,
    generate,
    approvedOutputs,
    openExport,
    cropDraft,
    setCropDraft,
    startCrop,
    cancelCrop,
    applyCrop,
    removeCrop,
    styleFilter,
    setStyleFilter,
    editingStyle,
    openStyleDialog,
    saveStyle,
    removeStyle,
  };
}

export type Studio = ReturnType<typeof useController>;

const StudioContext = createContext<Studio | null>(null);

export function StudioProvider({ children }: { children: ReactNode }) {
  return <StudioContext.Provider value={useController()}>{children}</StudioContext.Provider>;
}

export function useStudio(): Studio {
  const studio = useContext(StudioContext);
  if (!studio) throw new Error('useStudio must be used inside StudioProvider');
  return studio;
}
