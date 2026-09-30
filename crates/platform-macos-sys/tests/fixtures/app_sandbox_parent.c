#include <errno.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>

extern char **environ;

int main(int argc, char **argv) {
    if (argc != 6) {
        fprintf(stderr, "usage: parent CHILD RO RW DENIED PORT\n");
        return 64;
    }
    char *child_argv[] = {argv[1], argv[2], argv[3], argv[4], argv[5], NULL};
    pid_t pid = -1;
    int rc = posix_spawn(&pid, argv[1], NULL, NULL, child_argv, environ);
    if (rc != 0) {
        fprintf(stderr, "posix_spawn failed: %d\n", rc);
        return 65;
    }
    int status = 0;
    if (waitpid(pid, &status, 0) != pid) {
        perror("waitpid");
        return 66;
    }
    if (!WIFEXITED(status)) {
        fprintf(stderr, "child did not exit normally\n");
        return 67;
    }
    return WEXITSTATUS(status);
}
