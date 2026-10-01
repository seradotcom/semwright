#include <errno.h>
#include <stdio.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 6) {
        fprintf(stderr, "usage: wrapper PAYLOAD RO RW DENIED PORT\n");
        return 64;
    }
    char *payload_argv[] = {
        argv[1], argv[2], argv[3], argv[4], argv[5], NULL
    };
    execv(argv[1], payload_argv);
    perror("execv");
    return errno == 0 ? 65 : 66;
}
