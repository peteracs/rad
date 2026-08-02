/*
 * POSIX dirent implementation for TCC on Windows.
 * TCC doesn't link against MinGW's libmingwex which provides these.
 * Uses Win32 _findfirst/_findnext/_findclose underneath.
 *
 * We avoid the DIR struct from TCC's dirent.h entirely because:
 *  - dd_handle is long (32-bit on Win64) but _findfirst returns intptr_t (64-bit)
 *  - dd_name[1] is a flexible array that makes embedding DIR in a wrapper unsafe
 * Instead we define our own struct and cast through void*.
 */
#ifdef __TINYC__

#include <stdlib.h>
#include <string.h>
#include <io.h>
#include <stdint.h>

/* Pull in the dirent.h types but provide our own storage. */
#include <dirent.h>

typedef struct {
    struct _finddata_t dta;
    struct dirent ent;
    intptr_t handle;
    int started;
    char pattern[1024];
} MY_DIR;

DIR *opendir(const char *name) {
    if (!name || !*name) return NULL;
    MY_DIR *d = (MY_DIR *)calloc(1, sizeof(MY_DIR));
    if (!d) return NULL;

    size_t len = strlen(name);
    if (len > 0 && (name[len-1] == '/' || name[len-1] == '\\'))
        snprintf(d->pattern, sizeof(d->pattern), "%s*", name);
    else
        snprintf(d->pattern, sizeof(d->pattern), "%s\\*", name);

    /* Eagerly call _findfirst so that opendir() on a non-directory returns NULL. */
    d->handle = _findfirst(d->pattern, &d->dta);
    if (d->handle == -1) {
        free(d);
        return NULL;
    }
    d->started = 1;
    return (DIR *)d;
}

struct dirent *readdir(DIR *dir) {
    if (!dir) return NULL;
    MY_DIR *d = (MY_DIR *)dir;
    if (d->started == 1) {
        /* First readdir call — dta already filled by opendir's _findfirst. */
        d->started = 2;
    } else {
        if (_findnext(d->handle, &d->dta) != 0) return NULL;
    }
    d->ent.d_name = d->dta.name;
    d->ent.d_namlen = (unsigned short)strlen(d->dta.name);
    return &d->ent;
}

int closedir(DIR *dir) {
    if (!dir) return -1;
    MY_DIR *d = (MY_DIR *)dir;
    if (d->handle != -1) _findclose(d->handle);
    free(d);
    return 0;
}

#endif /* __TINYC__ */
