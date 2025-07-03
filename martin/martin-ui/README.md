# Martin Web UI

A web interface for previewing tiles served by Martin.

### Run locally

To run just the web interface

```bash
npm i
npm run dev
```
We also allow you to mock the requests which would go to the server via

```bash
NEXT_PUBLIC_MARTIN_ENABLE_MOCK_API=true npm run dev
```