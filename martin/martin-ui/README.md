# Martin Web UI

A web interface for previewing tiles served by Martin.

## Environment Variables

| Variable                             | Description                | Default     |
|--------------------------------------|----------------------------|-------------|
| `NEXT_PUBLIC_MARTIN_BASE`            | Martin API base URL        | UI origin   |
| `NEXT_PUBLIC_MARTIN_ENABLE_MOCK_API` | Enable mock API            | `false`     |
| `NEXT_PUBLIC_MARTIN_VERSION`         | App version                | `dev`       |

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

> [!TIP]
> Since the UI is served on port `:3001`, you will need to change either `NEXT_PUBLIC_MARTIN_BASE` or `NEXT_PUBLIC_MARTIN_ENABLE_MOCK_API`