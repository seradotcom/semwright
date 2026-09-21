/* Linux kernel contract test, NOT a test of compiled Rust. Uses only its temp tree. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/openat2.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>
static unsigned passed;
static void check(int condition,const char *name){if(!condition){fprintf(stderr,"FAIL %s (%s)\n",name,strerror(errno));exit(1);}printf("PASS %s\n",name);passed++;}
static int confined(int dir,const char*path){struct open_how how={.flags=O_RDONLY|O_CLOEXEC|O_NONBLOCK,.resolve=RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS|RESOLVE_NO_XDEV};return (int)syscall(SYS_openat2,dir,path,&how,sizeof(how));}
int main(int argc,char**argv){
    if(argc!=2){fprintf(stderr,"usage: openat2-test PRIVATE-TEMP-DIR\n");return 2;}
    int base=open(argv[1],O_DIRECTORY|O_RDONLY|O_CLOEXEC|O_NOFOLLOW);if(base<0)return 2;
    check(mkdirat(base,"root",0700)==0,"create confined root");
    int root=openat(base,"root",O_DIRECTORY|O_RDONLY|O_CLOEXEC|O_NOFOLLOW);check(root>=0,"open pinned root fd");
    int file=openat(root,"safe",O_CREAT|O_EXCL|O_WRONLY|O_CLOEXEC,0600);check(file>=0,"create fixture");check(write(file,"fixture",7)==7,"write fixture");close(file);
    int outside=openat(base,"outside",O_CREAT|O_EXCL|O_WRONLY,0600);check(outside>=0,"create outside sentinel");close(outside);
    file=confined(root,"safe");if(file<0&&errno==ENOSYS){puts("BLOCKED kernel lacks openat2");return 77;}check(file>=0,"ordinary relative file allowed");close(file);
    check(confined(root,"../outside")<0,"parent escape denied");
    check(confined(root,"/etc/passwd")<0,"absolute path denied before content read");
    check(symlinkat("../outside",root,"link")==0,"create fixture symlink");check(confined(root,"link")<0,"leaf symlink denied");
    check(symlinkat("..",root,"up")==0,"create parent symlink");check(confined(root,"up/outside")<0,"parent symlink denied");
    char proc[80];snprintf(proc,sizeof(proc),"/proc/self/fd/%d",base);check(symlinkat(proc,root,"magic")==0,"create magic-link fixture");check(confined(root,"magic/outside")<0,"magic-link escape denied");
    check(linkat(root,"safe",root,"hard",0)==0,"create hardlink fixture");file=confined(root,"hard");check(file>=0,"openat2 alone permits hardlinks");struct stat meta;check(fstat(file,&meta)==0&&meta.st_nlink==2,"additional single-link policy detects hardlink");close(file);
    check(renameat(base,"root",base,"moved")==0,"rename original root");check(mkdirat(base,"root",0700)==0,"replace root pathname");file=confined(root,"safe");check(file>=0,"pinned fd remains at original inode");close(file);
    close(root);close(base);printf("RESULT %u native kernel checks passed\n",passed);return 0;
}
