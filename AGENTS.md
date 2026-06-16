# Repository Guidelines

## Project Structure & Module Organization

This is a Tauri 2 desktop app with a React 19 + TypeScript frontend built by Vite. Frontend code lives in `src/`: `main.tsx` mounts the app, `App.tsx` contains the current UI, `App.css` holds styling, and `src/assets/` stores imported assets. Static files served by Vite belong in `public/`. Native Tauri code and configuration live in `src-tauri/`: Rust source is in `src-tauri/src/`, app permissions are in `src-tauri/capabilities/`, icons are in `src-tauri/icons/`, and packaging settings are in `src-tauri/tauri.conf.json`.

## Build, Test, and Development Commands

- `npm run dev`: start the Vite frontend on the fixed Tauri dev port, `1420`.
- `npm run tauri dev`: run the full desktop app with the Rust backend and Vite frontend.
- `npm run build`: run TypeScript checks with `tsc`, then create a production Vite build.
- `npm run preview`: preview the built frontend locally.
- `npm run tauri build`: create a packaged Tauri application.
- `cd src-tauri && cargo test`: run Rust tests when backend code is added or changed.

## Coding Style & Naming Conventions

Use TypeScript with strict compiler settings. Keep React components in PascalCase, hooks and functions in camelCase, and CSS classes lowercase with semantic names. Match the existing frontend style: two-space indentation, double quotes, semicolons, and explicit imports. Rust code should follow `rustfmt` defaults, four-space indentation, snake_case functions, and clear `#[tauri::command]` boundaries.

## Testing Guidelines

No JavaScript test framework is configured yet. Until one is added, treat `npm run build` as the required frontend verification step. If UI tests are introduced, place them near the related component as `*.test.tsx` and prefer Vitest or React Testing Library. For Rust changes, add unit tests in the relevant `src-tauri/src/*.rs` module and run `cargo test`.

## Commit & Pull Request Guidelines

The current history uses a Conventional Commit style, for example `feat(init): setup project`. Continue with `type(scope): summary`, such as `fix(tauri): validate greet input` or `chore(deps): update vite`. Pull requests should include a short description, testing performed, linked issues when applicable, and screenshots or recordings for visible UI changes. Call out any permission, capability, packaging, or installer changes under `src-tauri/`.

## Security & Configuration Tips

Keep Tauri capabilities minimal in `src-tauri/capabilities/default.json`. Do not commit secrets, signing keys, generated installers, or local environment files. Document any required environment variables in `README.md` before relying on them in code.
