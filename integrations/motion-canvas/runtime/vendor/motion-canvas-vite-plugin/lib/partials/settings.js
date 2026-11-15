"use strict";
// Semwright adaptation of @motion-canvas/vite-plugin 3.17.2 (MIT).
Object.defineProperty(exports, "__esModule", {value: true});
exports.settingsPlugin = settingsPlugin;
const fs = require("node:fs");
const path = require("node:path");
const {randomUUID} = require("node:crypto");
const LIMIT = 1024 * 1024;
function samePath(a, b) {
    return process.platform === "win32" ? a.toLowerCase() === b.toLowerCase() : a === b;
}
function sameFile(a, b) {
    return a.dev === b.dev && a.ino === b.ino && a.mode === b.mode &&
        a.size === b.size && a.mtimeMs === b.mtimeMs && a.nlink === b.nlink;
}
function settingsPlugin({settingsFile}) {
    if (typeof settingsFile !== "string" || !/^[A-Za-z0-9_.-]+$/.test(settingsFile) ||
        settingsFile === "." || settingsFile === ".." ||
        /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(settingsFile) ||
        settingsFile.endsWith(".")) {
        throw new Error("settingsFile must be an explicit ordinary filename inside the project root");
    }
    const settingsId = "virtual:settings.meta";
    const resolvedSettingsId = "\0" + settingsId;
    let root;
    let rootIdentity;
    async function checkRoot() {
        if (!root) throw new Error("settings root is not configured");
        const parts = root.slice(path.parse(root).root.length).split(path.sep).filter(Boolean);
        let current = path.parse(root).root;
        for (const part of parts) {
            current = path.join(current, part);
            const item = await fs.promises.lstat(current);
            if (item.isSymbolicLink() || !item.isDirectory()) throw new Error("settings root contains a link or non-directory");
        }
        const currentIdentity = await fs.promises.lstat(root);
        if (currentIdentity.dev !== rootIdentity.dev || currentIdentity.ino !== rootIdentity.ino ||
            !samePath(await fs.promises.realpath(root), root)) {
            throw new Error("settings root identity changed");
        }
    }
    async function observe() {
        await checkRoot();
        try {
            const item = await fs.promises.lstat(path.join(root, settingsFile));
            if (!item.isFile() || item.isSymbolicLink() || item.nlink !== 1 || item.size > LIMIT) {
                throw new Error("settings must be one bounded ordinary file");
            }
            return item;
        } catch (error) {
            if (error.code === "ENOENT") return null;
            throw error;
        }
    }
    async function read() {
        const before = await observe();
        if (before === null) return {text: "", identity: null};
        const file = await fs.promises.open(path.join(root, settingsFile),
            fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
        try {
            const opened = await file.stat();
            if (!opened.isFile() || !sameFile(before, opened)) throw new Error("settings changed before open");
            const buffer = Buffer.alloc(LIMIT + 1);
            let length = 0;
            while (length < buffer.length) {
                const result = await file.read(buffer, length, buffer.length - length, length);
                if (result.bytesRead === 0) break;
                length += result.bytesRead;
            }
            const after = await file.stat();
            const named = await observe();
            if (length > LIMIT || named === null || !sameFile(opened, after) || !sameFile(before, named)) {
                throw new Error("settings changed while reading");
            }
            return {text: buffer.subarray(0, length).toString("utf8"), identity: named};
        } finally {
            await file.close();
        }
    }
    async function write(data) {
        const text = JSON.stringify(data, undefined, 2);
        if (typeof text !== "string" || Buffer.byteLength(text) > LIMIT) throw new Error("settings data exceeds bounds");
        const previous = await read();
        if (previous.text === text) return false;
        const temporary = path.join(root, ".semwright-settings-" + randomUUID() + ".tmp");
        let created = false;
        let failure;
        try {
            await checkRoot();
            const file = await fs.promises.open(temporary, "wx", 0o600);
            created = true;
            try {
                await file.writeFile(text, "utf8");
                await file.sync();
            } finally {
                await file.close();
            }
            const now = await observe();
            if ((now === null) !== (previous.identity === null) ||
                (now !== null && !sameFile(now, previous.identity))) {
                throw new Error("settings changed before replacement");
            }
            await fs.promises.rename(temporary, path.join(root, settingsFile));
            created = false;
            const retained = await read();
            if (retained.text !== text) throw new Error("settings replacement readback differs");
            return true;
        } catch (error) {
            failure = error;
            throw error;
        } finally {
            if (created) {
                try { await fs.promises.unlink(temporary); }
                catch (error) { if (failure) failure.cleanupError = error; else throw error; }
            }
        }
    }
    return {
        name: "motion-canvas:settings",
        async configResolved(config) {
            if (typeof config.root !== "string" || !path.isAbsolute(config.root)) throw new Error("settings root must be absolute");
            root = path.resolve(config.root);
            rootIdentity = await fs.promises.lstat(root);
            await checkRoot();
        },
        resolveId(id) {
            if (id === settingsId) return resolvedSettingsId;
        },
        async load(id) {
            if (id !== resolvedSettingsId) return;
            const current = await read();
            return JSON.stringify(current.identity === null ? {} : JSON.parse(current.text));
        },
        configureServer(server) {
            server.ws.on("motion-canvas:meta", async ({source, data}, client) => {
                if (source !== resolvedSettingsId) return;
                if (await write(data)) {
                    const module = await server.moduleGraph.getModuleByUrl(source);
                    if (module) server.moduleGraph.invalidateModule(module);
                }
                client.send("motion-canvas:meta-ack", {source});
            });
        },
    };
}
