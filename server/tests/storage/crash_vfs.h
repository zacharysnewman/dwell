#pragma once

// A test VFS simulating a crash (or power cut) mid-save: it wraps the platform's default VFS and,
// once its write budget is spent, drops every later change to files — writes, truncations, syncs
// and deletes fail as I/O errors and never reach the disk. Reopening the files with the default VFS
// then shows what a restart after the crash would find.
#include <sqlite3.h>

#include <cstring>

namespace dwell::test {

struct CrashVfs {
  static inline int writes_left = -1;  // −1: unlimited
  static inline bool crashed = false;

  static void Arm(int budget) {
    writes_left = budget;
    crashed = false;
  }
  static void Disarm() { writes_left = -1; }

  static bool Spend() {
    if (crashed) return false;
    if (writes_left < 0) return true;
    if (writes_left == 0) {
      crashed = true;
      return false;
    }
    --writes_left;
    return true;
  }

  struct File {
    sqlite3_file base;
    sqlite3_file* real;
  };
  static sqlite3_file* Real(sqlite3_file* f) { return reinterpret_cast<File*>(f)->real; }

  static int Close(sqlite3_file* f) {
    const int rc = Real(f)->pMethods ? Real(f)->pMethods->xClose(Real(f)) : SQLITE_OK;
    sqlite3_free(Real(f));
    return rc;
  }
  static int Read(sqlite3_file* f, void* p, int n, sqlite3_int64 o) {
    return Real(f)->pMethods->xRead(Real(f), p, n, o);
  }
  static int Write(sqlite3_file* f, const void* p, int n, sqlite3_int64 o) {
    return Spend() ? Real(f)->pMethods->xWrite(Real(f), p, n, o) : SQLITE_IOERR_WRITE;
  }
  static int Truncate(sqlite3_file* f, sqlite3_int64 size) {
    return Spend() ? Real(f)->pMethods->xTruncate(Real(f), size) : SQLITE_IOERR_TRUNCATE;
  }
  static int Sync(sqlite3_file* f, int flags) {
    return crashed ? SQLITE_IOERR_FSYNC : Real(f)->pMethods->xSync(Real(f), flags);
  }
  static int FileSize(sqlite3_file* f, sqlite3_int64* size) {
    return Real(f)->pMethods->xFileSize(Real(f), size);
  }
  static int Lock(sqlite3_file* f, int l) { return Real(f)->pMethods->xLock(Real(f), l); }
  static int Unlock(sqlite3_file* f, int l) { return Real(f)->pMethods->xUnlock(Real(f), l); }
  static int CheckReservedLock(sqlite3_file* f, int* out) {
    return Real(f)->pMethods->xCheckReservedLock(Real(f), out);
  }
  static int FileControl(sqlite3_file* f, int op, void* arg) {
    return Real(f)->pMethods->xFileControl(Real(f), op, arg);
  }
  static int SectorSize(sqlite3_file* f) { return Real(f)->pMethods->xSectorSize(Real(f)); }
  static int DeviceCharacteristics(sqlite3_file* f) {
    return Real(f)->pMethods->xDeviceCharacteristics(Real(f));
  }
  static int ShmMap(sqlite3_file* f, int page, int size, int extend, void volatile** out) {
    return Real(f)->pMethods->xShmMap(Real(f), page, size, extend, out);
  }
  static int ShmLock(sqlite3_file* f, int offset, int n, int flags) {
    return Real(f)->pMethods->xShmLock(Real(f), offset, n, flags);
  }
  static void ShmBarrier(sqlite3_file* f) { Real(f)->pMethods->xShmBarrier(Real(f)); }
  static int ShmUnmap(sqlite3_file* f, int del) {
    return Real(f)->pMethods->xShmUnmap(Real(f), crashed ? 0 : del);
  }

  static sqlite3_vfs* Default() {
    static sqlite3_vfs* base = sqlite3_vfs_find(nullptr);
    return base;
  }

  static int Open(sqlite3_vfs*, sqlite3_filename name, sqlite3_file* f, int flags, int* out) {
    static sqlite3_io_methods v1 = [] {
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
    static sqlite3_io_methods v2 = [] {
      sqlite3_io_methods m = v1;
      m.iVersion = 2;
      m.xShmMap = ShmMap;
      m.xShmLock = ShmLock;
      m.xShmBarrier = ShmBarrier;
      m.xShmUnmap = ShmUnmap;
      return m;
    }();
    auto* file = reinterpret_cast<File*>(f);
    file->base.pMethods = nullptr;
    file->real = static_cast<sqlite3_file*>(sqlite3_malloc(Default()->szOsFile));
    if (!file->real) return SQLITE_NOMEM;
    std::memset(file->real, 0, static_cast<std::size_t>(Default()->szOsFile));
    const int rc = Default()->xOpen(Default(), name, file->real, flags, out);
    if (rc != SQLITE_OK) {
      sqlite3_free(file->real);
      return rc;
    }
    file->base.pMethods = file->real->pMethods->iVersion >= 2 ? &v2 : &v1;
    return SQLITE_OK;
  }
  static int Delete(sqlite3_vfs*, const char* name, int sync) {
    return Spend() ? Default()->xDelete(Default(), name, sync) : SQLITE_IOERR_DELETE;
  }
  static int Access(sqlite3_vfs*, const char* name, int flags, int* out) {
    return Default()->xAccess(Default(), name, flags, out);
  }
  static int FullPathname(sqlite3_vfs*, const char* name, int n, char* out) {
    return Default()->xFullPathname(Default(), name, n, out);
  }
  static int Randomness(sqlite3_vfs*, int n, char* out) {
    return Default()->xRandomness(Default(), n, out);
  }
  static int Sleep(sqlite3_vfs*, int us) { return Default()->xSleep(Default(), us); }
  static int CurrentTime(sqlite3_vfs*, double* out) {
    return Default()->xCurrentTime(Default(), out);
  }

  // Registers the VFS as "crash" (once).
  static void Register() {
    static sqlite3_vfs vfs = [] {
      sqlite3_vfs v{};
      v.iVersion = 1;
      v.szOsFile = sizeof(File);
      v.mxPathname = Default()->mxPathname;
      v.zName = "crash";
      v.xOpen = Open;
      v.xDelete = Delete;
      v.xAccess = Access;
      v.xFullPathname = FullPathname;
      v.xRandomness = Randomness;
      v.xSleep = Sleep;
      v.xCurrentTime = CurrentTime;
      return v;
    }();
    static const bool registered = (sqlite3_vfs_register(&vfs, 0), true);
    (void)registered;
  }
};

}  // namespace dwell::test
