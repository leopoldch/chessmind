from setuptools import setup
from Cython.Build import cythonize
import numpy as np

setup(
    ext_modules=cythonize([
        'engine_cython/speedups.pyx',
        'engine_cython/eval_speedups.pyx',
        'engine_cython/search_speedups.pyx',
    ], language_level=3),
    include_dirs=[np.get_include()],
)
