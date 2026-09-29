// Node stand-in for the browser's OPFS files (server/core/src/storage/opfs_vfs.cpp): the WASM
// storage tests read and write real files through the same `dwellFiles` interface.
Module['dwellFiles'] = (() => {
  const fs = require('fs');
  const fds = new Map();
  let next = 1;
  const fd = (id) => fds.get(id);
  return {
    open(name, create) {
      try {
        const exists = fs.existsSync(name);
        if (!exists && !create) return -1;
        const id = next++;
        fds.set(id, fs.openSync(name, exists ? 'r+' : 'w+'));
        return id;
      } catch (e) {
        return -1;
      }
    },
    read: (id, bytes, offset) => fs.readSync(fd(id), bytes, 0, bytes.length, offset),
    write: (id, bytes, offset) => {
      fs.writeSync(fd(id), bytes, 0, bytes.length, offset);
    },
    size: (id) => fs.fstatSync(fd(id)).size,
    truncate: (id, size) => fs.ftruncateSync(fd(id), size),
    flush: (id) => fs.fsyncSync(fd(id)),
    close(id) {
      fs.closeSync(fd(id));
      fds.delete(id);
    },
    exists: (name) => fs.existsSync(name),
    remove(name) {
      try {
        fs.unlinkSync(name);
      } catch (e) {}
    },
  };
})();
