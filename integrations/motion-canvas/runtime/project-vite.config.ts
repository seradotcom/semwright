import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {defineConfig} from 'vite';
import {verifyExportedMotionCanvasPlugin} from './plugin-integrity.mjs';

const root = fileURLToPath(new URL('.', import.meta.url));
const motionPluginEntry = import.meta.resolve('@semwright/motion-canvas-vite-plugin');
await verifyExportedMotionCanvasPlugin(
  root,
  await readFile(new URL('./package-lock.json', import.meta.url)),
  motionPluginEntry,
);
const {default: motionCanvasModule} = await import(motionPluginEntry);
const motionCanvas = typeof motionCanvasModule === 'function' ? motionCanvasModule : motionCanvasModule.default;
export default defineConfig({
  plugins: [motionCanvas({project: './src/project.ts', settingsFile: '.semwright-settings.json'})],
});
