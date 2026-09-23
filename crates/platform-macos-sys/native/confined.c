/* macOS descriptor confinement. Portable POSIX subset tested on Linux.
 * Scope is a pinned root and ONE child. Multi-component grants fail closed:
 * openat component walks do not reproduce openat2 RESOLVE_BENEATH under rename.
 * The pinned root remains the granted object if renamed. No path re-resolution.
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#ifndef _DARWIN_C_SOURCE
#define _DARWIN_C_SOURCE
#endif
#include "confined.h"
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

static int io_error(void) { return errno==ENOENT ? SW_NOT_FOUND : SW_DENIED; }
static int child_name(const char *name) {
    if (!name || !*name || strlen(name)>255 || strcmp(name,".")==0 || strcmp(name,"..")==0) return SW_INVALID;
    return strchr(name,'/') ? SW_UNSUPPORTED : 0;
}
static int good_root(int fd, struct stat *s) {
    if (fstat(fd,s)<0 || !S_ISDIR(s->st_mode) || s->st_uid!=getuid() || (s->st_mode & 0022)) return SW_DENIED;
    return 0;
}
static int regular(int fd, const struct stat *root, size_t limit) {
    struct stat st;
    if (fstat(fd,&st)<0 || !S_ISREG(st.st_mode) || st.st_nlink!=1 || st.st_dev!=root->st_dev) return SW_DENIED;
    if (st.st_size<0 || (uint64_t)st.st_size>limit) return SW_BUDGET;
    return 0;
}
int sw_root_open(const char *absolute) {
    if (!absolute || absolute[0]!='/' || absolute[1]=='\0' || strlen(absolute)>4096) return SW_INVALID;
    char path[4097]; memcpy(path,absolute,strlen(absolute)+1);
    int fd=open("/",O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
    if(fd<0) return SW_IO;
    char *p=path+1;
    while (*p) {
        char *slash=strchr(p,'/'); if(slash) *slash='\0';
        if (!*p || strcmp(p,".")==0 || strcmp(p,"..")==0) { close(fd); return SW_INVALID; }
        int next=openat(fd,p,O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
        if(next<0) { int err=io_error(); close(fd); return err; }
        close(fd); fd=next;
        if(!slash) break;
        p=slash+1; if(!*p) { close(fd); return SW_INVALID; }
    }
    struct stat st; if(good_root(fd,&st)!=0) { close(fd); return SW_DENIED; }
    return fd;
}
int sw_child_read_open(int root, const char *child, size_t limit) {
    int status=child_name(child); if(status) return status;
    if(!limit || limit>1048576) return SW_INVALID;
    struct stat rs; if(good_root(root,&rs)) return SW_DENIED;
    int fd=openat(root,child,O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC);
    if(fd<0) return io_error();
    status=regular(fd,&rs,limit);
    if(status) { close(fd); return status; }
    return fd;
}
int sw_child_write_atomic(int root, const char *child, const char *temporary, const unsigned char *data, size_t size) {
    int status=child_name(child); if(status) return status;
    if(child_name(temporary) || strncmp(temporary,".semwright-",11)!=0 || strcmp(temporary,child)==0) return SW_INVALID;
    if(size>1048576 || (!data && size)) return SW_BUDGET;
    struct stat rs; if(good_root(root,&rs)) return SW_DENIED;
    int existing=openat(root,child,O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC);
    if(existing>=0) {
        status=regular(existing,&rs,SIZE_MAX); close(existing); if(status) return status;
    } else if(errno!=ENOENT) { return SW_DENIED; }
    int fd=openat(root,temporary,O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC,0600);
    if(fd<0) return io_error();
    status=regular(fd,&rs,SIZE_MAX);
    size_t offset=0;
    while(!status && offset<size) {
        ssize_t n=write(fd,data+offset,size-offset);
        if(n<0 && errno==EINTR) continue;
        if(n<=0) { status=SW_IO; break; }
        offset+=(size_t)n;
    }
    if(!status && fsync(fd)<0) status=SW_IO;
    close(fd);
    /* renameat never follows a replaced destination symlink. At worst it replaces
       the directory entry inside the granted root. No bytes leave the root. */
    if(!status && renameat(root,temporary,root,child)<0) status=SW_IO;
    if(status) { (void)unlinkat(root,temporary,0); return status; }
    /* Data may already be committed: never label post-rename fsync failure safe to retry. */
    if(fsync(root)<0) return SW_UNCERTAIN;
    return 0;
}
