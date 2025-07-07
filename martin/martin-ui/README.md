# Martin Web UI

A web interface for previewing tiles served by Martin.

## Environment Variables

| Variable                             | Description                | Default     |
|--------------------------------------|----------------------------|-------------|
| `VITE_MARTIN_BASE`                   | Martin API base URL        | UI origin   |
| `VITE_MARTIN_VERSION`                | App version                | `dev`       |

## Configuration

1. Copy `.env` to `.env.local`:
   ```bash
   cp .env .env.local
   ```

2. Configure the environment variables in `.env.local` for your setup.

## Run locally

To run just the web interface:

```bash
npm i
npm run dev
```

> [!IMPORTANT]
> Since the UI is served on port `:3001`, you will need to change `VITE_MARTIN_BASE` to point to your Martin instance
