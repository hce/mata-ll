-- A where-bound name inside a top-level VALUE binding is local to that
-- binding's emitted closure. Regression: the value-binding emission
-- kept the where names registered as Lua locals after the binding
-- (restore_keeping_locals keeps the module scope as grown), so a
-- where name that collides with a top-level binding shadowed the
-- top-level's fn-table slot in every LATER binding — `go def` below
-- emitted the bare name `def` (a nil global) instead of the slot
-- reference, and calling it crashed at runtime.

def :: Int -> Int
def x = x + 1

-- registers where-local `def` while emitting this CAF
first :: Int
first = def 10
  where def y = y * 2

-- emitted AFTER the collision: `def` must resolve to the TOP-LEVEL def
applied :: Int
applied = go def
  where go f = f 6

-- same collision inside an IO value binding's where block
report :: IO ()
report = printIt (def 1)
  where printIt v = print v
        def y = y * 100

after :: Int
after = def 30

main :: IO ()
main = do
    print first
    print applied
    report
    print after
