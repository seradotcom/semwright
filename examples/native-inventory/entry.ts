import { bridgeEntrypoint } from '../../sdk/native-typescript/src/index.js';
import { Inventory } from './application.js';

// The executable entry is owner-pinned and bundled ahead of installation.
// The JSON operation frame never contains code, executable paths, or grants.
export const semwrightNativeBridgeMain = bridgeEntrypoint((paths, readOnly) =>
  new Inventory(paths.data_root, readOnly),
);
