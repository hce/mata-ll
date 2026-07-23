-- ghc_regr004: Record update preserving fields from original

data Config = Config
    { cfgHost    :: String
    , cfgPort    :: Int
    , cfgDebug   :: Bool
    , cfgTimeout :: Int
    , cfgMaxConn :: Int
    } deriving (Show, Eq)

defaultConfig :: Config
defaultConfig = Config { cfgHost = "localhost", cfgPort = 8080, cfgDebug = False, cfgTimeout = 30, cfgMaxConn = 100 }

data Point3D = Point3D { px :: Number, py :: Number, pz :: Number }
    deriving (Show, Eq)

origin :: Point3D
origin = Point3D { px = 0.0, py = 0.0, pz = 0.0 }

main :: IO ()
main = do
    -- Single field update; rest preserved
    let prod = defaultConfig { cfgHost = "prod.example.com" }
    assert (cfgHost prod == "prod.example.com") "host updated"
    assert (cfgPort prod == 8080) "port preserved"
    assert (cfgDebug prod == False) "debug preserved"
    assert (cfgTimeout prod == 30) "timeout preserved"
    assert (cfgMaxConn prod == 100) "maxconn preserved"

    -- Multiple field update
    let debug = defaultConfig { cfgDebug = True, cfgPort = 9090, cfgTimeout = 5 }
    assert (cfgDebug debug == True) "debug set"
    assert (cfgPort debug == 9090) "port set"
    assert (cfgTimeout debug == 5) "timeout set"
    assert (cfgHost debug == "localhost") "host preserved after multi-update"
    assert (cfgMaxConn debug == 100) "maxconn preserved after multi-update"

    -- Original unchanged
    assert (cfgHost defaultConfig == "localhost") "original host"
    assert (cfgPort defaultConfig == 8080) "original port"

    -- Chained updates
    let p1 = origin { px = 1.0 }
    let p2 = p1 { py = 2.0 }
    let p3 = p2 { pz = 3.0 }
    assert (px p3 == 1.0) "px chained"
    assert (py p3 == 2.0) "py chained"
    assert (pz p3 == 3.0) "pz chained"
    assert (px origin == 0.0) "origin unchanged"

    -- Equality after update
    let same = defaultConfig { cfgPort = 8080 }
    assert (same == defaultConfig) "same after no-op update"

    putStrLn "ok"
