# dchat Landing Site

A modern landing page for dchat - the decentralized, end-to-end encrypted chat platform.

## 🛠️ Tech Stack

- **Next.js 15.3** - React framework with App Router
- **React 19** - UI library
- **Tailwind CSS 4** - Utility-first CSS
- **Framer Motion** - Animations
- **TypeScript** - Type safety

## 🚀 Project Structure

```text
landing/
├── public/          # Static assets (images, fonts)
├── src/
│   ├── app/         # Next.js App Router pages
│   │   ├── page.tsx           # Home page
│   │   ├── layout.tsx         # Root layout
│   │   ├── blockchain/        # Blockchain info page
│   │   ├── developers/        # Developer docs page
│   │   └── tokenomics/        # Token economics page
│   ├── components/  # React components
│   │   ├── Header.tsx
│   │   ├── Footer.tsx
│   │   ├── Hero.tsx
│   │   ├── Features.tsx
│   │   └── ...
│   ├── content/     # Static content/copy
│   ├── data/        # Mock data for components
│   └── animations/  # Lottie animations
├── package.json
├── next.config.ts
├── tailwind.config.ts
└── tsconfig.json
```

## 🧞 Commands

All commands are run from the `landing/` directory:

| Command           | Action                                      |
| :---------------- | :------------------------------------------ |
| `npm install`     | Installs dependencies                       |
| `npm run dev`     | Starts dev server at `localhost:3000`       |
| `npm run build`   | Build production site to `.next/`           |
| `npm run start`   | Start production server                     |
| `npm run lint`    | Run ESLint                                  |

## 🔗 Backend Integration (TODO)

The landing site is currently static. Future integration points:

- **Node Health API**: Display network status from `/health` endpoint
- **Metrics API**: Show live stats from `/metrics` endpoint
- **Network Stats**: Real-time peer count, message throughput
- **Chain Explorer**: Link to block explorer for chat chain

## 📦 Deployment

Build and deploy as a static site or with Node.js:

```bash
npm run build
npm run start
```

Or export as static HTML:

```bash
npm run build
# Output in .next/ or use next export
```
