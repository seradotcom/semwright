#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

static int denied_errno(void) { return errno == EACCES || errno == EPERM; }

static int read_exact_file(const char *path, const char *wanted) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) return 0;
    char buf[128] = {0};
    ssize_t n = read(fd, buf, sizeof(buf) - 1);
    close(fd);
    return n >= 0 && strcmp(buf, wanted) == 0;
}

static int must_fail_read(const char *path) {
    errno = 0;
    int fd = open(path, O_RDONLY);
    if (fd >= 0) { close(fd); return 0; }
    return denied_errno();
}

static int must_fail_write(const char *path) {
    errno = 0;
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd >= 0) { close(fd); unlink(path); return 0; }
    return denied_errno();
}

static int must_write(const char *path) {
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0) return 0;
    const char payload[] = "written";
    ssize_t n = write(fd, payload, sizeof(payload) - 1);
    close(fd);
    return n == (ssize_t)(sizeof(payload) - 1);
}

static int network_must_be_denied(unsigned short port) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return denied_errno();
    struct sockaddr_in address;
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons(port);
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    errno = 0;
    int rc = connect(fd, (struct sockaddr *)&address, sizeof(address));
    int saved = errno;
    close(fd);
    if (rc == 0) return 0;
    errno = saved;
    return denied_errno();
}

int main(int argc, char **argv) {
    if (argc != 5) return 64;
    char ro_input[4096], ro_write[4096], rw_write[4096], denied_input[4096];
    if (snprintf(ro_input, sizeof(ro_input), "%s/input.txt", argv[1]) >= (int)sizeof(ro_input) ||
        snprintf(ro_write, sizeof(ro_write), "%s/blocked.txt", argv[1]) >= (int)sizeof(ro_write) ||
        snprintf(rw_write, sizeof(rw_write), "%s/output.txt", argv[2]) >= (int)sizeof(rw_write) ||
        snprintf(denied_input, sizeof(denied_input), "%s/secret.txt", argv[3]) >= (int)sizeof(denied_input)) return 65;
    if (!read_exact_file(ro_input, "allowed-ro")) return 10;
    if (!must_fail_write(ro_write)) return 11;
    if (!must_write(rw_write)) return 12;
    if (!must_fail_read(denied_input)) return 13;
    char *end = NULL;
    unsigned long raw_port = strtoul(argv[4], &end, 10);
    if (!end || *end || raw_port == 0 || raw_port > 65535) return 14;
    if (!network_must_be_denied((unsigned short)raw_port)) return 15;
    puts("macos-app-sandbox-probe: PASS");
    return 0;
}
