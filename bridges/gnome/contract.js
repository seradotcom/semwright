/* Pure shared contract. No shell, eval, dynamic property assignment or script source. */
function validateOperation(raw) {
    if (typeof raw !== 'string' || raw.length > 8192) throw new Error('InvalidArgument');
    const value = JSON.parse(raw);
    if (!value || Array.isArray(value) || typeof value !== 'object') throw new Error('InvalidArgument');
    const allowed = ['id', 'operation', 'target', 'fingerprint', 'x', 'y', 'width', 'height'];
    if (Object.keys(value).some(key => !allowed.includes(key))) throw new Error('InvalidArgument');
    if (!['focus', 'move', 'resize', 'close'].includes(value.operation)) throw new Error('Unsupported');
    for (const key of ['id', 'target', 'fingerprint']) {
        if (typeof value[key] !== 'string' || value[key].length < 1 || value[key].length > 256)
            throw new Error('InvalidArgument');
    }
    for (const key of value.operation === 'move' ? ['x', 'y'] : value.operation === 'resize' ? ['width', 'height'] : []) {
        if (!Number.isInteger(value[key])) throw new Error('InvalidArgument');
        const minimum = value.operation === 'resize' ? 1 : -32768;
        const maximum = value.operation === 'resize' ? 16384 : 32767;
        if (value[key] < minimum || value[key] > maximum) throw new Error('InvalidArgument');
    }
    return value;
}
function exactWindow(windows, command, describe) {
    const found = windows.filter(window => String(describe(window).id) === command.target);
    if (found.length !== 1) throw new Error(found.length ? 'AmbiguousTarget' : 'StaleReference');
    if (describe(found[0]).fingerprint !== command.fingerprint) throw new Error('StaleReference');
    return found[0];
}
export {validateOperation, exactWindow};
