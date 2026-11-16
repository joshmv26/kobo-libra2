/* Fallback getauxval() for the old Kobo sysroot (glibc < 2.16). Weak: a real libc one is used if present. */
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>

__attribute__((weak)) unsigned long getauxval(unsigned long type) {
	unsigned long buf[2];
	int fd = open("/proc/self/auxv", O_RDONLY);
	if (fd < 0) { errno = ENOENT; return 0; }
	while (read(fd, buf, sizeof(buf)) == (long)sizeof(buf)) {
		if (buf[0] == 0) break;
		if (buf[0] == type) { close(fd); return buf[1]; }
	}
	close(fd);
	errno = ENOENT;
	return 0;
}
