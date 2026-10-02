module ScopeMid (helper, viaInner) where

import ScopeInner (other)

helper :: Int -> Int
helper x = x + 1

viaInner :: Int -> Int
viaInner x = other x
