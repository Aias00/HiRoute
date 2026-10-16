# Native x64 Release dependencies use the same static CRT as the Rust images.
if(NOT CMAKE_HOST_WIN32)
  message(FATAL_ERROR "The Windows Release toolchain requires a Windows host")
endif()
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded" CACHE STRING "HiRoute x64 Release CRT" FORCE)
