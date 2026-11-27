# CloudMCP

## Verification

- `pnpm build` runs the type check and the production bundle.
- `node --test 'scripts/*.test.mjs'` runs the tests for the pure modules in `src/lib`. The tests import the TypeScript sources directly, so imports between those modules need the `.ts` extension, and the modules cannot use `import.meta.env` or Tauri APIs.
- `cd src-tauri && cargo test` runs the backend tests. `cargo check` also validates `tauri.conf.json` and the capability file.
- To look at the UI without the desktop app, run `pnpm exec vite --port <free port>` and open `/?preview` for the populated fixture in `src/lib/preview.ts`, or `/` for the empty state.

## The running instance

The installed copy at `~/Applications/CloudMCP.app` is usually running and serving remote agents on `127.0.0.1:8787`. A second instance with the same identifier shares its data directory, and quitting either instance cancels every active execution.

- Start `pnpm tauri dev` or a built bundle only under a different identifier, for example `--config '{"identifier":"dev.cloudmcp.desktop.verify"}'`, so that it gets its own data directory.
- Leave port 8787 and `~/Library/Application Support/dev.cloudmcp.desktop` alone.
- Copy a build into `~/Applications` only when the user asks for it. An install is: build with `pnpm tauri build --bundles app --config src-tauri/tauri.macos-bundle.conf.json`, replace `~/Applications/CloudMCP.app` with the new bundle without keeping a backup of the old one, then delete the bundle under `src-tauri/target/`.
- Keep `~/Applications/CloudMCP.app` the only `CloudMCP.app` on the machine. Spotlight opens whichever same-named copy it has launched most, which used to be a stale `target/debug` bundle. `mdfind "kMDItemCFBundleIdentifier == 'dev.cloudmcp.desktop*'"` lists every copy.

## Frontend

The UI follows Bento's H1 style (`~/Software/apphub`). Tokens and the `h1-*` classes are in `src/index.css`, and the page layout classes are in `src/components/dashboard.css`. Tailwind 4 puts utilities in a later cascade layer than these classes, so change how a primitive looks on one element with utility classes on that element.
