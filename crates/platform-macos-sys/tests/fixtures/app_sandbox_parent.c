#include <errno.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>

extern char **environ;

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--smoke") == 0) {
        puts("sandbox-parent-smoke: PASS");
        return 0;
    }
    if (argc != 6 && argc != 7) {
        fprintf(stderr, "usage: parent CHILD [PAYLOAD] RO RW DENIED PORT\n");
        return 64;
    }
    char *direct_argv[] = {argv[1], argv[2], argv[3], argv[4], argv[5], NULL};
    char *exec_argv[] = {argv[1], argv[2], argv[3], argv[4], argv[5], argv[6], NULL};
    char **child_argv = argc == 7 ? exec_argv : direct_argv;
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
