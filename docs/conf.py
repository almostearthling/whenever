# Configuration file for the Sphinx documentation builder.
#
# For the full list of built-in configuration values, see the documentation:
# https://www.sphinx-doc.org/en/master/usage/configuration.html

# -- Project information -----------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#project-information

project = "The Whenever Scheduler"
author = "Francesco Garosi"
copyright = "2023-%Y, Francesco Garosi"
release = "0.4.8"

html_logo = "graphics/metronome.png"

# -- General configuration ---------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#general-configuration

extensions = ['myst_parser', 'sphinx_favicon']
source_suffix = ['.rst', '.md']

# templates_path = ['_templates']
exclude_patterns = ['_*.rst', '_*.md', '_build', 'Thumbs.db', '.DS_Store']

# -- Options for HTML output -------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#options-for-html-output

html_theme = 'sphinx_rtd_theme'
html_static_path = ['static']

favicons = ['favicon.ico']

myst_heading_anchors = 3

# end.
