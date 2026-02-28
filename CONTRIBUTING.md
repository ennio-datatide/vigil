# Contributing to Praefectus

Thanks for your interest in contributing!

## Getting Started

```bash
git clone https://github.com/ennio-datatide/praefectus.git
cd praefectus
npm install
npm run build
npm test
```

### Prerequisites

- Node.js 22+
- npm
- tmux
- Claude Code CLI (`claude`)

## Development

```bash
npm run dev      # Start dev servers (Fastify on :4000, Next.js on :3000)
npm run build    # Build all workspaces
npm test         # Run all tests
npm run check    # Lint and format check
npm run format   # Auto-format all files
```

## Making Changes

1. Fork the repo and create a branch from `main`
2. Make your changes
3. Add or update tests as needed
4. Run `npm test` and `npm run check` to verify
5. Open a pull request

## Project Structure

- `apps/server` — Fastify 5 backend
- `apps/web` — Next.js 15 frontend
- `packages/shared` — Shared Zod schemas and types
- `cli` — Commander.js CLI

## Code Style

This project uses [Biome](https://biomejs.dev/) for linting and formatting. Run `npm run check` before submitting a PR.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
