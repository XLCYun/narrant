"""直接配置 CMake 导入目标，锁定官方 LLVM Windows DIA 绝对路径的修复范围。"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
CMAKE = os.environ.get('NARRANT_CMAKE') or shutil.which('cmake')
OLD_LIBRARY = 'C:/Program Files (x86)/Microsoft Visual Studio/2019/Professional/DIA SDK/lib/amd64/diaguids.lib'


class WindowsDiaImportTests(unittest.TestCase):
    """只验证真正的 CMake target 属性，不伪称本机执行过 Windows linker。"""

    def configure(self, *, library_exists=True, old_library=OLD_LIBRARY):
        """用上游 LLVMDebugInfoPDB 的真实接口形状配置导入目标。"""
        self.assertTrue(CMAKE, 'Set NARRANT_CMAKE to an available CMake executable')
        with tempfile.TemporaryDirectory(prefix='narrant-dia-import-') as directory:
            root = Path(directory)
            vs = root / 'Visual Studio 2022 Enterprise'
            library = vs / 'DIA SDK/lib/amd64/diaguids.lib'
            if library_exists:
                library.parent.mkdir(parents=True)
                library.write_bytes(b'test-only-existence-sentinel')
            source = root / 'source'
            source.mkdir()
            module = ROOT / 'cmake/RelocateWindowsDia.cmake'
            # 假库只用于 CMake 属性验证；不运行编译器、不下载 SDK、不执行被分析源码。
            (source / 'CMakeLists.txt').write_text(f'''
cmake_minimum_required(VERSION 3.24)
project(dia_import_regression LANGUAGES NONE)
add_library(LLVMDebugInfoPDB INTERFACE IMPORTED)
set(_original "LLVMObject;{old_library};LLVMSupport;unrelated/diaguids.lib")
set_target_properties(LLVMDebugInfoPDB PROPERTIES INTERFACE_LINK_LIBRARIES "${{_original}}")
if(EXISTS "{module.as_posix()}")
  include("{module.as_posix()}")
  narrant_relocate_windows_dia("{vs.as_posix()}")
endif()
get_target_property(_actual LLVMDebugInfoPDB INTERFACE_LINK_LIBRARIES)
set(_expected "LLVMObject;{library.as_posix()};LLVMSupport;unrelated/diaguids.lib")
if(NOT _actual STREQUAL _expected)
  message(FATAL_ERROR "DIA import still uses the upstream build host: ${{_actual}}")
endif()
''', encoding='utf-8')
            return subprocess.run([CMAKE, '-S', str(source), '-B', str(root / 'build')],
                                  capture_output=True, text=True, timeout=30)

    def test_replaces_only_the_pinned_imported_dependency(self):
        """固定旧 DIA 路径被替换，链接顺序与所有其它依赖原样保留。"""
        result = self.configure()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('Rebound LLVM 21.1.8 DIA dependency', result.stdout)

    def test_missing_current_dia_library_is_a_configuration_error(self):
        """批准的当前 DIA 库不存在时，配置阶段失败而不是再尝试旧路径。"""
        result = self.configure(library_exists=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Required VS2022 DIA library is missing', result.stderr)

    def test_unexpected_export_is_not_rewritten_broadly(self):
        """上游图不再含固定路径时明确失败，不通过文件名匹配篡改其它库。"""
        result = self.configure(old_library='C:/unexpected-sdk/diaguids.lib')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Expected one pinned LLVM 21.1.8 DIA dependency', result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
