-- Deliberately ill-typed import target for
-- diagnostics_name_the_imported_file (compile_errors.rs): the type error
-- below sits INSIDE an imported module, so its diagnostic must name this
-- file (and excerpt this line), not render a bare line:col that the reader
-- would chase into the root file.
--
-- This directory is not under tests/cases/ on purpose: lua-compat.sh
-- compiles everything there and treats a compile error as a failure, and
-- the case-registry / oracle-registry completeness tests would each demand
-- a registration for it.
module BrokenIntHelper where

broken :: Int
broken = "not an int"
