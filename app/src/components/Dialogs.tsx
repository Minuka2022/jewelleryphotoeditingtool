// Pop-up windows: settings, style import, shortcuts, confirmations and
// the message stack in the corner.

import { useEffect, useState } from 'react';
import { api, errorText } from '../api';
import { useStudio } from '../store';
import type { Look, PromptCheck, Tone } from '../types';
import { Button, Field, Modal, cx, inputClass } from '../ui';

function SettingsDialog() {
  const { settings, setSettings, setDialog } = useStudio();
  const [key, setKey] = useState('');
  const [working, setWorking] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const close = () => setDialog(null);

  const attempt = async (action: () => Promise<string>) => {
    setWorking(true);
    setMessage(null);
    try {
      setMessage({ ok: true, text: await action() });
    } catch (error) {
      setMessage({ ok: false, text: errorText(error) });
    } finally {
      setWorking(false);
    }
  };

  const save = () =>
    attempt(async () => {
      setSettings(await api.setApiKey(key));
      setKey('');
      return `Key saved. ${await api.checkApiKey()}`;
    });

  const remove = () =>
    attempt(async () => {
      setSettings(await api.setApiKey(''));
      return 'The saved key was removed.';
    });

  return (
    <Modal title="Settings" onClose={close} footer={<Button onClick={close}>Done</Button>}>
      <Field
        label="Gemini API key"
        hint="Get a key at aistudio.google.com. It is stored in the Windows Credential Manager on this computer and sent only to Google. Image models need billing turned on for the key's Google project."
      >
        <input
          type="password"
          autoFocus
          className={inputClass}
          placeholder={settings?.hasApiKey ? 'A key is saved. Paste a new one to replace it.' : 'Paste your key here'}
          value={key}
          onChange={(event) => setKey(event.target.value)}
        />
      </Field>
      <div className="flex gap-2">
        <Button variant="primary" disabled={working || !key.trim()} onClick={save}>
          Save key
        </Button>
        <Button disabled={working || !settings?.hasApiKey} onClick={() => attempt(api.checkApiKey)}>
          Test saved key
        </Button>
        <Button disabled={working || !settings?.hasApiKey} onClick={remove}>
          Remove key
        </Button>
      </div>
      {working && <p className="text-dim">Checking with Google...</p>}
      {message && <p className={message.ok ? 'text-good' : 'text-bad'}>{message.text}</p>}
    </Modal>
  );
}

const tones: [Tone, string][] = [
  ['bright', 'Bright scene'],
  ['neutral', 'Neutral'],
  ['dark', 'Dark scene'],
];

const finishes: [Look, string, string][] = [
  ['photo', 'Photograph', 'Looks like a real studio photograph.'],
  ['render', '3D render (CAD look)', 'Flawless computer-rendered catalogue look: mirror-smooth metal, razor-sharp stones.'],
];

/** Adds a new style, or edits `editingStyle` when one is set. */
function StyleDialog() {
  const { library, activeItem, saveStyle, setDialog, editingStyle: editing } = useStudio();
  const [look, setLook] = useState<Look>(editing?.look ?? 'photo');
  const [name, setName] = useState(editing?.name ?? '');
  const [description, setDescription] = useState(editing?.description ?? '');
  const [prompt, setPrompt] = useState(editing?.prompt ?? '');
  const [tone, setTone] = useState<Tone>(editing?.tone ?? 'neutral');
  // A new style starts with the open photo's type ticked, the usual intent.
  const [categories, setCategories] = useState<string[]>(
    editing ? editing.categories : activeItem && activeItem.category !== 'jewelry' ? [activeItem.category] : [],
  );
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [check, setCheck] = useState<PromptCheck | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const close = () => setDialog(null);

  // Check the wording shortly after the user stops typing.
  useEffect(() => {
    if (!prompt.trim()) {
      setCheck(null);
      return;
    }
    let current = true;
    const timer = setTimeout(() => {
      api
        .checkStylePrompt(prompt)
        .then((result) => current && setCheck(result))
        .catch(() => current && setCheck(null));
    }, 350);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [prompt]);

  const toggleCategory = (id: string) =>
    setCategories((list) => (list.includes(id) ? list.filter((other) => other !== id) : [...list, id]));

  const chooseImage = async () => {
    try {
      setImagePath((await api.pickImage()) ?? imagePath);
    } catch (pickError) {
      setError(errorText(pickError));
    }
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    const problem = await saveStyle({ name, description, prompt, tone, look, categories, imagePath });
    setSaving(false);
    if (problem) setError(problem);
    else close();
  };

  return (
    <Modal
      title={editing ? 'Edit Style' : 'Import Style'}
      width="w-[560px]"
      onClose={close}
      footer={
        <>
          {editing && (
            <span className="mr-auto self-center text-[11px] text-dim">
              Version {editing.version}. Changing the prompt or the finish makes it version {editing.version + 1}.
            </span>
          )}
          <Button onClick={close}>Cancel</Button>
          <Button variant="primary" disabled={saving || !name.trim() || !prompt.trim()} onClick={save}>
            {editing ? 'Save changes' : 'Save style'}
          </Button>
        </>
      }
    >
      <Field label="Style name">
        <input
          autoFocus
          className={inputClass}
          placeholder="e.g. Rose Petals"
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
      </Field>

      <Field
        label="Example image (optional)"
        hint="A picture of the look. It becomes the style's thumbnail so you can recognise it. It is not sent to the AI."
      >
        <div className="flex items-center gap-2">
          <Button onClick={chooseImage}>Choose image...</Button>
          <span className="truncate text-dim">
            {imagePath
              ? imagePath.replace(/^.*[\\/]/, '')
              : editing?.thumbnail
                ? 'Keeping the current picture'
                : 'No image chosen'}
          </span>
        </div>
      </Field>

      <Field
        label="Prompt"
        hint="Write anything you like. Write {item} where the jewelry goes, or leave it out and it is added for you. The app's own rules still ask the AI to keep your piece exactly as it is."
      >
        <textarea
          className={cx(inputClass, 'h-32 resize-y leading-relaxed')}
          placeholder="Place the {item} on scattered pale pink rose petals over ivory silk. Soft window light from the left, gentle shadows, romantic mood."
          value={prompt}
          onChange={(event) => setPrompt(event.target.value)}
        />
      </Field>
      {check && (check.warnings.length > 0 || check.itemWords.length > 0) && (
        <div className="rounded border border-[#c9a23f]/50 bg-[#c9a23f]/10 px-3 py-2 leading-snug text-[#e3c16a]">
          <strong>Tip, not a rule:</strong> some words here can make the AI change the piece itself, not just
          the scene.
          <ul className="mt-1 list-disc pl-4">
            {check.warnings.map((warning) => (
              <li key={warning}>{warning}</li>
            ))}
            {check.itemWords.length > 0 && (
              <li>
                {check.itemWords.join(', ')} describe jewelry. If they mean your piece, writing {'{item}'} instead
                is safer.
              </li>
            )}
          </ul>
          You can still save the style as it is.
        </div>
      )}

      <Field label="Finish" hint={finishes.find(([id]) => id === look)?.[2]}>
        <div className="flex flex-wrap gap-1.5">
          {finishes.map(([id, label]) => (
            <button
              key={id}
              type="button"
              aria-pressed={look === id}
              onClick={() => setLook(id)}
              className={cx(
                'rounded-full border px-2.5 py-0.5',
                look === id ? 'border-accent bg-accent/25 text-ink' : 'border-edge text-dim hover:text-ink',
              )}
            >
              {label}
            </button>
          ))}
        </div>
      </Field>

      <Field label="Use this style for" hint="Leave everything off to use it for every type of jewelry.">
        <div className="flex flex-wrap gap-1.5">
          {library?.categories
            .filter((category) => category.id !== 'jewelry')
            .map((category) => {
              const on = categories.includes(category.id);
              return (
                <button
                  key={category.id}
                  type="button"
                  aria-pressed={on}
                  onClick={() => toggleCategory(category.id)}
                  className={cx(
                    'rounded-full border px-2.5 py-0.5',
                    on ? 'border-accent bg-accent/25 text-ink' : 'border-edge text-dim hover:text-ink',
                  )}
                >
                  {category.label}
                </button>
              );
            })}
        </div>
      </Field>

      <div className="grid grid-cols-[1fr_150px] gap-3">
        <Field label="Short description (optional)">
          <input
            className={inputClass}
            placeholder="Shown when you hover over the style"
            value={description}
            onChange={(event) => setDescription(event.target.value)}
          />
        </Field>
        <Field label="Scene brightness">
          <select
            className={cx(inputClass, 'h-[26px] py-0')}
            value={tone}
            onChange={(event) => setTone(event.target.value as Tone)}
          >
            {tones.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </Field>
      </div>

      {error && <p className="text-bad">{error}</p>}
    </Modal>
  );
}

const shortcuts: [string, string][] = [
  ['Ctrl + O', 'Open photos'],
  ['Ctrl + G', 'Generate'],
  ['Ctrl + Shift + E', 'Export approved images'],
  ['Left / Right', 'Previous / next image'],
  ['A', 'Approve the image (press again to undo)'],
  ['X', 'Reject the image (press again to undo)'],
  ['Delete', 'Delete the image'],
  ['1  2  3', 'Generated only / side by side / slider'],
  ['0 or double-click', 'Fit to window'],
  ['Mouse wheel', 'Zoom in and out'],
  ['H  Z', 'Hand tool / zoom tool'],
];

function ShortcutsDialog() {
  const { setDialog } = useStudio();
  const close = () => setDialog(null);
  return (
    <Modal title="Keyboard Shortcuts" onClose={close} footer={<Button onClick={close}>Close</Button>}>
      <table className="w-full">
        <tbody>
          {shortcuts.map(([keys, action]) => (
            <tr key={keys} className="border-b border-line last:border-0">
              <td className="py-1.5 pr-4 font-semibold whitespace-nowrap">{keys}</td>
              <td className="py-1.5 text-dim">{action}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Modal>
  );
}

function ConfirmDialog() {
  const { confirmation, setConfirmation } = useStudio();
  if (!confirmation) return null;
  const close = () => setConfirmation(null);
  return (
    <Modal
      title={confirmation.title}
      width="w-[400px]"
      onClose={close}
      footer={
        <>
          <Button onClick={close}>Cancel</Button>
          <Button
            variant={confirmation.danger ? 'danger' : 'primary'}
            autoFocus
            onClick={() => {
              close();
              confirmation.run();
            }}
          >
            {confirmation.action}
          </Button>
        </>
      }
    >
      <p className="leading-relaxed text-dim">{confirmation.body}</p>
    </Modal>
  );
}

function Toasts() {
  const { toasts, dismissToast } = useStudio();
  return (
    <div className="fixed right-3 bottom-9 z-50 flex w-[380px] flex-col gap-2">
      {toasts.map((toast) => (
        <button
          key={toast.id}
          type="button"
          title="Click to dismiss"
          onClick={() => dismissToast(toast.id)}
          className={cx(
            'rounded border bg-panel px-3 py-2 text-left leading-snug shadow-xl select-text',
            toast.kind === 'error' ? 'border-bad text-ink' : 'border-edge text-ink',
          )}
        >
          {toast.kind === 'error' && <span className="mr-1 font-semibold text-bad">Problem:</span>}
          {toast.text}
        </button>
      ))}
    </div>
  );
}

export function Dialogs() {
  const { dialog } = useStudio();
  return (
    <>
      {dialog === 'settings' && <SettingsDialog />}
      {dialog === 'import-style' && <StyleDialog />}
      {dialog === 'shortcuts' && <ShortcutsDialog />}
      <ConfirmDialog />
      <Toasts />
    </>
  );
}
