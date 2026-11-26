//===-- measure_exec.c - Isolate benchmark child from Python ------*- C -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Forks a distinct benchmark child so wait4's ru_maxrss measures only that
// child. The launcher itself is a small C binary; a Python parent would
// otherwise contribute its resident pages to the child's pre-exec peak.
//
// Prints one line: exit_code wall_seconds user_seconds sys_seconds maxrss_kib
//
//===----------------------------------------------------------------------===//

#define _GNU_SOURCE

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static double to_seconds(const struct timespec *ts) {
  return (double)ts->tv_sec + (double)ts->tv_nsec / 1000000000.0;
}

static double timeval_seconds(const struct timeval *tv) {
  return (double)tv->tv_sec + (double)tv->tv_usec / 1000000.0;
}

int main(int argc, char **argv) {
  if (argc < 2) {
    fprintf(stderr, "usage: measure_exec COMMAND [ARG...]\n");
    return 2;
  }

  struct timespec start;
  if (clock_gettime(CLOCK_MONOTONIC, &start) != 0) {
    perror("clock_gettime");
    return 2;
  }

  pid_t pid = fork();
  if (pid < 0) {
    perror("fork");
    return 2;
  }
  if (pid == 0) {
    int devnull = open("/dev/null", O_WRONLY);
    if (devnull >= 0) {
      dup2(devnull, STDOUT_FILENO);
      dup2(devnull, STDERR_FILENO);
      if (devnull > STDERR_FILENO)
        close(devnull);
    }
    execvp(argv[1], &argv[1]);
    _exit(127);
  }

  int status;
  struct rusage usage;
  if (wait4(pid, &status, 0, &usage) < 0) {
    perror("wait4");
    return 2;
  }

  struct timespec end;
  if (clock_gettime(CLOCK_MONOTONIC, &end) != 0) {
    perror("clock_gettime");
    return 2;
  }

  int exit_code;
  if (WIFEXITED(status))
    exit_code = WEXITSTATUS(status);
  else if (WIFSIGNALED(status))
    exit_code = -WTERMSIG(status);
  else
    exit_code = 255;

  printf("%d %.9f %.9f %.9f %ld\n", exit_code,
         to_seconds(&end) - to_seconds(&start),
         timeval_seconds(&usage.ru_utime), timeval_seconds(&usage.ru_stime),
         usage.ru_maxrss);
  return 0;
}
