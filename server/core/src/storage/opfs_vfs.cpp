// SQLite VFS for the browser build (ARCHITECTURE.md §6.4, ADR 0006): files are
// FileSystemSyncAccessHandles from the Origin Private File System, opened by the hosting worker
// before the world is (client/src/local/worldFiles.ts) and reached through `Module.dwellFiles`:
//   open(name, create) → id or −1, read(id, bytes, offset) → count, write(id, bytes, offset),
//   size(id), truncate(id, size), flush(id), exists(name), remove(name).
// Synchronous access handles need no cross-origin isolation. There is no shared memory, so the
// world file uses exclusive locking and a rollback journal (WorldDb OpenOptions::wal = false);
// locks are no-ops (one connection per file). Node test builds provide `dwellFiles` over `fs`.
#include <emscripten/emscripten.h>
#include <sqlite3.h>

#include <cstring>

// The JavaScript bodies below are not C++: keep clang-format out of them.
// clang-format off
EM_JS_DEPS(dwell_vfs, "$UTF8ToString");

EM_JS(int, dwell_file_open, (const char* name, int create), {
  const f = Module['dwellFiles'];
  return f ? f.open(UTF8ToString(name), create ? true : false) : -1;
});
EM_JS(int, dwell_file_read, (int id, void* buf, int n, double offset), {
  try {
    return Module['dwellFiles'].read(id, HEAPU8.subarray(buf, buf + n), offset);
  } catch (e) {
    return -1;
  }
});
EM_JS(int, dwell_file_write, (int id, const void* buf, int n, double offset), {
  try {
    Module['dwellFiles'].write(id, HEAPU8.subarray(buf, buf + n), offset);
    return 0;
  } catch (e) {
    return -1;
  }
});
EM_JS(double, dwell_file_size, (int id), { return Module['dwellFiles'].size(id); });
EM_JS(void, dwell_file_truncate, (int id, double size),
      { Module['dwellFiles'].truncate(id, size); });
EM_JS(void, dwell_file_flush, (int id), { Module['dwellFiles'].flush(id); });
EM_JS(void, dwell_file_close, (int id), {
  const f = Module['dwellFiles'];
  if (f.close) f.close(id);
});
EM_JS(int, dwell_file_exists, (const char* name), {
  const f = Module['dwellFiles'];
  return f && f.exists(UTF8ToString(name)) ? 1 : 0;
});
EM_JS(void, dwell_file_remove, (const char* name), {
  const f = Module['dwellFiles'];
  if (f) f.remove(UTF8ToString(name));
});

EM_JS(void, dwell_random, (char* out, int n),
      { crypto.getRandomValues(HEAPU8.subarray(out, out + n)); });
EM_JS(double, dwell_date_now, (), { return Date.now(); });
// clang-format on

namespace {

struct File {
  sqlite3_file base;
  int id;
};

int Id(sqlite3_file* f) { return reinterpret_cast<File*>(f)->id; }

// JS exceptions (a closed handle, a full disk) surface as I/O errors.
int Close(sqlite3_file* f) {
  dwell_file_close(Id(f));
  return SQLITE_OK;
}

int Read(sqlite3_file* f, void* out, int n, sqlite3_int64 offset) {
  const int got = dwell_file_read(Id(f), out, n, static_cast<double>(offset));
  if (got < 0) return SQLITE_IOERR_READ;
  if (got < n) {
    std::memset(static_cast<char*>(out) + got, 0, static_cast<std::size_t>(n - got));
    return SQLITE_IOERR_SHORT_READ;
  }
  return SQLITE_OK;
}

int Write(sqlite3_file* f, const void* data, int n, sqlite3_int64 offset) {
  return dwell_file_write(Id(f), data, n, static_cast<double>(offset)) == 0 ? SQLITE_OK
                                                                            : SQLITE_IOERR_WRITE;
}

int Truncate(sqlite3_file* f, sqlite3_int64 size) {
  dwell_file_truncate(Id(f), static_cast<double>(size));
  return SQLITE_OK;
}

int Sync(sqlite3_file* f, int) {
  dwell_file_flush(Id(f));
  return SQLITE_OK;
}

int FileSize(sqlite3_file* f, sqlite3_int64* size) {
  *size = static_cast<sqlite3_int64>(dwell_file_size(Id(f)));
  return SQLITE_OK;
}

int Lock(sqlite3_file*, int) { return SQLITE_OK; }
int Unlock(sqlite3_file*, int) { return SQLITE_OK; }
int CheckReservedLock(sqlite3_file*, int* out) {
  *out = 0;
  return SQLITE_OK;
}
int FileControl(sqlite3_file*, int, void*) { return SQLITE_NOTFOUND; }
int SectorSize(sqlite3_file*) { return 4096; }
int DeviceCharacteristics(sqlite3_file*) { return SQLITE_IOCAP_SAFE_APPEND; }

const sqlite3_io_methods kMethods = [] {
  sqlite3_io_methods m{};
  m.iVersion = 1;
  m.xClose = Close;
  m.xRead = Read;
  m.xWrite = Write;
  m.xTruncate = Truncate;
  m.xSync = Sync;
  m.xFileSize = FileSize;
  m.xLock = Lock;
  m.xUnlock = Unlock;
  m.xCheckReservedLock = CheckReservedLock;
  m.xFileControl = FileControl;
  m.xSectorSize = SectorSize;
  m.xDeviceCharacteristics = DeviceCharacteristics;
  return m;
}();

int Open(sqlite3_vfs*, sqlite3_filename name, sqlite3_file* f, int flags, int* out_flags) {
  auto* file = reinterpret_cast<File*>(f);
  file->base.pMethods = nullptr;
  if (!name) return SQLITE_CANTOPEN;  // temporary files: SQLITE_TEMP_STORE=3 keeps them in memory
  const int id = dwell_file_open(name, (flags & SQLITE_OPEN_CREATE) != 0 ? 1 : 0);
  if (id < 0) return SQLITE_CANTOPEN;
  file->id = id;
  file->base.pMethods = &kMethods;
  if (out_flags) *out_flags = flags;
  return SQLITE_OK;
}

int Delete(sqlite3_vfs*, const char* name, int) {
  dwell_file_remove(name);
  return SQLITE_OK;
}

int Access(sqlite3_vfs*, const char* name, int, int* out) {
  *out = dwell_file_exists(name);
  return SQLITE_OK;
}

int FullPathname(sqlite3_vfs*, const char* name, int n, char* out) {
  sqlite3_snprintf(n, out, "%s", name);
  return SQLITE_OK;
}

int Randomness(sqlite3_vfs*, int n, char* out) {
  dwell_random(out, n);
  return n;
}

int Sleep(sqlite3_vfs*, int us) { return us; }  // single connection: nothing to wait for

int CurrentTime(sqlite3_vfs*, double* out) {
  *out = 2440587.5 + dwell_date_now() / 86400000.0;
  return SQLITE_OK;
}

sqlite3_vfs g_vfs = [] {
  sqlite3_vfs v{};
  v.iVersion = 1;
  v.szOsFile = sizeof(File);
  v.mxPathname = 1024;
  v.zName = "dwell-opfs";
  v.xOpen = Open;
  v.xDelete = Delete;
  v.xAccess = Access;
  v.xFullPathname = FullPathname;
  v.xRandomness = Randomness;
  v.xSleep = Sleep;
  v.xCurrentTime = CurrentTime;
  return v;
}();

}  // namespace

// SQLITE_OS_OTHER: the build has no built-in OS layer; this is it.
extern "C" int sqlite3_os_init() { return sqlite3_vfs_register(&g_vfs, /*makeDflt=*/1); }
extern "C" int sqlite3_os_end() { return SQLITE_OK; }
