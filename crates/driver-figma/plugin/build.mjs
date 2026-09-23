import {mkdir, readFile, writeFile} from 'node:fs/promises';
await mkdir(new URL('./dist/', import.meta.url), {recursive:true});
await writeFile(new URL('./dist/ui.html', import.meta.url), await readFile(new URL('./src/ui.html', import.meta.url)));
console.log('plugin asset build PASS');
