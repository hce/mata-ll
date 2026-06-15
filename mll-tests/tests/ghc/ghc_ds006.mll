-- GHC ds006: Record syntax
-- Tests record construction, field access, and record update

data Config = Config
    { host :: String
    , port :: Integer
    , debug :: Bool
    }
    deriving (Show, Eq)

data Vec2 = Vec2
    { vx :: Number
    , vy :: Number
    }
    deriving (Show, Eq)

addVec :: Vec2 -> Vec2 -> Vec2
addVec a b = Vec2 { vx = vx a + vx b, vy = vy a + vy b }

scaleVec :: Number -> Vec2 -> Vec2
scaleVec s v = Vec2 { vx = s * vx v, vy = s * vy v }

main :: IO ()
main = do
    let cfg = Config { host = "localhost", port = 8080, debug = False }
    assert (host cfg == "localhost") "host"
    assert (port cfg == 8080) "port"
    assert (debug cfg == False) "debug"

    -- Record update
    let cfg2 = cfg { port = 9090, debug = True }
    assert (host cfg2 == "localhost") "update preserves"
    assert (port cfg2 == 9090) "update changes port"
    assert (debug cfg2 == True) "update changes debug"

    -- Original unchanged
    assert (port cfg == 8080) "original unchanged"

    -- Vec operations
    let v1 = Vec2 { vx = 1.0, vy = 2.0 }
    let v2 = Vec2 { vx = 3.0, vy = 4.0 }
    assert (addVec v1 v2 == Vec2 { vx = 4.0, vy = 6.0 }) "addVec"
    assert (scaleVec 2.0 v1 == Vec2 { vx = 2.0, vy = 4.0 }) "scaleVec"

    putStrLn "ok"
