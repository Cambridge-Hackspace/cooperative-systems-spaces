# edge apparatus Frontend

Modern Vue 3 + TypeScript frontend for the edge apparatus status and management interface.

## Features

- 📱 Device status dashboard
- 🌐 Web-based device registration
- 💻 CLI registration instructions
- 🔄 Real-time status updates (polls every 30s)
- 🎨 Modern UI with Tailwind CSS and DaisyUI
- 📊 System information display
- ⚡ Fast Vite build system

## Tech Stack

- **Vue 3** - Progressive JavaScript framework
- **TypeScript** - Type-safe development
- **Vite** - Next generation frontend tooling
- **Tailwind CSS** - Utility-first CSS framework
- **DaisyUI** - Beautiful component library
- **Pinia** - State management
- **Vue Router** - Client-side routing
- **Axios** - HTTP client

## Development

Install dependencies:

```bash
npm install
```

Start development server (runs on http://localhost:5174):

```bash
npm run dev
```

Build for production:

```bash
npm run build
```

The built files will be in the `dist/` directory.

## Project Structure

```
frontend_edge/
├── src/
│   ├── api/           # API client functions
│   ├── components/    # Reusable Vue components
│   ├── router/        # Vue Router configuration
│   ├── stores/        # Pinia stores
│   ├── types/         # TypeScript type definitions
│   ├── views/         # Page components
│   ├── App.vue        # Root component
│   ├── main.ts        # Application entry point
│   └── style.css      # Global styles
├── index.html         # HTML entry point
├── package.json       # Dependencies and scripts
├── tailwind.config.js # Tailwind configuration
├── tsconfig.json      # TypeScript configuration
└── vite.config.ts     # Vite configuration
```

## API Endpoints

The frontend expects these API endpoints from the edge apparatus:

- `GET /api/status` - Get current device status
- `POST /api/register` - Register device with Space Server

## Deployment

To deploy to the edge apparatus:

1. Build the frontend:

   ```bash
   npm run build
   ```

2. Copy the `dist/` directory contents to the edge apparatus's static file location

3. Update the edge apparatus server to serve these files

## Configuration

The Vite dev server proxies API requests to `http://localhost:8080` (the edge apparatus default port).

To change the API proxy target, edit `vite.config.ts`:

```typescript
server: {
  proxy: {
    '/api': {
      target: 'http://your-edge-device:port',
      changeOrigin: true,
    }
  }
}
```
