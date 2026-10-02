#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 5) return 64;
    char marker[4096];
    if (snprintf(marker, sizeof(marker), "%s/exec-marker.txt", argv[2]) >= (int)sizeof(marker)) {
        return 65;
    }
    int fd = open(marker, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0) return 66;
    const char payload[] = "executed";
    ssize_t written = write(fd, payload, sizeof(payload) - 1);
    close(fd);
    return written == (ssize_t)(sizeof(payload) - 1) ? 0 : 67;
}
