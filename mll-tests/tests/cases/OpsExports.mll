-- Helper module for import_operator_list.mll: exports two operators and
-- a function, imported through operator-carrying import/hiding lists.
module OpsExports ((&), (|>), pipeApply) where

(&) :: a -> (a -> b) -> b
x & f = f x

(|>) :: Int -> Int -> Int
a |> b = a * 10 + b

pipeApply :: Int -> Int
pipeApply n = n & (\v -> v + 1)
