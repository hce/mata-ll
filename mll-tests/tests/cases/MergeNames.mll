-- Helper module for import_merge.mll: three exported names, imported by
-- the main case through several import declarations of THIS one module
-- (two Specific lists, a qualified alias, and a hiding+specific pair in
-- the compile-error tests). Repeated imports must merge like GHC's.
module MergeNames (alpha, beta, gamma) where

alpha :: Int
alpha = 1

beta :: Int
beta = 2

gamma :: Int
gamma = 3
