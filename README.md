# Jewelry Studio

A Windows desktop app that turns ordinary inventory photos of jewelry into
studio-style product photos. You open a photo, pick one or more output styles,
and Google's Gemini image model generates the new pictures. You then compare
each result with the original, approve the good ones and export them.

## Install and run

1. Double-click `installer/JewelryStudio-<version>-setup.exe` (or download it
   from the repository's Releases page).
   Windows may show "Windows protected your PC" because the installer is not
   code-signed yet: choose **More info**, then **Run anyway**.
2. Start **Jewelry Studio** from the Start menu.
3. The first time, open **File > Settings** and paste your Gemini API key
   (get one at <https://aistudio.google.com>). Image models have no free
   tier, so the key's Google project needs billing turned on.

## Using it

1. **Open photos**: drop them onto the window, or File > Open Photos.
2. **Set the type** (Ring, Necklace, ...) in the Properties panel.
3. **Tick one or more styles** in the Styles panel.
4. **Generate**. The button shows how many images and what they will cost.
5. **Review**: click a result in the filmstrip. Use side by side (2) or the
   slider (3), and the mouse wheel to zoom into stones and prongs.
6. **Approve** the faithful ones (A), **reject** (X) or delete the rest.
7. **File > Export Approved** copies the approved images to a folder.

Help > Keyboard Shortcuts lists every shortcut.

### Adding your own styles

Styles panel > **Import style**. Give it a name, optionally an example picture
(used as the thumbnail only), choose which jewelry types it is for, and write
the prompt. **Finish** chooses between a result that looks like a real
photograph and one with the flawless look of a 3D CAD render. Describe only
the scene (surface, background, props, lighting,
mood) and write `{item}` where the jewelry goes. Wording that would change the
jewelry itself (a different angle, worn on a hand, added stones) is refused.

### Where things are kept

`%LOCALAPPDATA%\com.idearigs.jewelrystudio` holds your photos, the generated
images, your imported styles and settings. The API key is stored in the
Windows Credential Manager, not in a file.

### Updates

Each time it starts, the app checks the repository's Releases page. When a
newer version is there, a bar appears with **Update and restart**. Help >
Check for Updates does the same on demand. Updates are signed, and the app
refuses any file that was not signed with this project's key.

## Things to know

- **The model redraws the whole image.** Nothing guarantees it copies the
  jewelry perfectly, so every result needs a human check against the
  reference. The compare views exist for that check.
- **Photos are sent to Google with storage turned off**, so Google does not
  keep them after the request.
- **Model names and prices are data**, in `library/pricing.toml`. Google
  changes both often; update that file when they do and rebuild.

## How a request is built

| Layer | Where it lives | Purpose |
|---|---|---|
| Global rules | `library/master.toml` `[global]` | Reproduce the item exactly; change only the scene |
| Category | `library/master.toml` `[[category]]` | Which details matter for a ring, a necklace, ... |
| Style | `library/styles/<id>.toml`, or imported in the app | Surface, background, props, lighting, mood |
| Note | The note box next to Generate | Optional small adjustment for one run |

The files in `library/` are the built-in set. They are compiled into the app
and refreshed in the data folder on every start, so edits belong here in the
repository, followed by a rebuild.

## For developers

```
crates/studio-core/   prompt library, guardrails, photo preparation, cost, Gemini client
crates/poc-cli/       command-line test tool (batch runs with an HTML report)
app/src/              the interface (React, TypeScript, Tailwind)
app/src-tauri/        the desktop shell and backend commands (Tauri, Rust)
library/              built-in master prompt, styles and prices
scripts/              build-installer.ps1
```

```
cargo test --workspace                         # all Rust tests
cd app; npm run build; npm run preview         # interface in a browser, with placeholder data
powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1   # build the installer
cargo run -p poc-cli -- --help                 # the command-line tool
```

### Releasing a new version

```
powershell -ExecutionPolicy Bypass -File scripts\release.ps1 -Version 0.3.0 -Notes "What changed"
```

This sets the version (one place: `[workspace.package]` in the root
`Cargo.toml`), builds and signs the installer, pushes the code with a version
tag, and publishes a GitHub release containing the installer and the
`latest.json` file that installed copies read. Commit your changes first.

The signing key is at `%USERPROFILE%\.tauri\jewelry-studio.key`, outside the
repository. Back it up: without it, installed copies will reject future
updates and everyone would have to reinstall by hand. Never commit it.

The app can only read releases anonymously, so the repository that holds the
releases must be public.

Opening the interface in a normal browser uses a stand-in backend with
placeholder pictures (add `?demo` to the address for sample content), so the
interface can be worked on without spending money on generations.

Useful environment variables for testing the desktop app:
`JEWELRY_STUDIO_DATA_DIR` (use another data folder), `GEMINI_BASE_URL` (send
requests to a local test server) and `GEMINI_API_KEY` (used when no key is
saved).

On this project's first machine Rust uses the GNU toolchain plus the WinLibs
MinGW package; `scripts/build-installer.ps1` contains the two workarounds
that setup needs. With the Visual Studio C++ Build Tools and the MSVC
toolchain, neither is needed.
