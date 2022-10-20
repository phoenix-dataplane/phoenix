add_executable(test_mt_index test_mt_index.cc)
include_directories(test_mt_index ${mt_index_INCLUDE_DIR_L})
link_directories(test_mt_index ${mt_index_LIBRARY_DIRS_L})
target_link_libraries(test_mt_index ${mt_index_LINKER_LIBS_L})
