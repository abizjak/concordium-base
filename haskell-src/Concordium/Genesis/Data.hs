{-# LANGUAGE DataKinds #-}
{-# LANGUAGE ExistentialQuantification #-}
{-# LANGUAGE ScopedTypeVariables #-}
{-# LANGUAGE TypeApplications #-}
{-# LANGUAGE TypeFamilies #-}

module Concordium.Genesis.Data (
    GenesisBaker (..),
    GenesisAccount (..),
    module Concordium.Genesis.Data.Base,
    module Concordium.Genesis.Data,
) where

import Data.Function (on)
import Data.Serialize

import Concordium.Common.Version
import Concordium.Genesis.Account
import Concordium.Genesis.Data.Base
import qualified Concordium.Genesis.Data.P1 as P1
import qualified Concordium.Genesis.Data.P2 as P2
import Concordium.Types

-- |Data family for genesis data.
-- This has been chosen to be a data family so that the genesis data
-- will uniquely determine the protocol version.
data family GenesisData (pv :: ProtocolVersion)

newtype instance GenesisData 'P1 = GDP1 {unGDP1 :: P1.GenesisDataP1}
newtype instance GenesisData 'P2 = GDP2 {unGDP2 :: P2.GenesisDataP2}

instance (IsProtocolVersion pv) => BasicGenesisData (GenesisData pv) where
    gdGenesisTime = case protocolVersion @pv of
        SP1 -> gdGenesisTime . unGDP1
        SP2 -> gdGenesisTime . unGDP2
    {-# INLINE gdGenesisTime #-}
    gdSlotDuration = case protocolVersion @pv of
        SP1 -> gdSlotDuration . unGDP1
        SP2 -> gdSlotDuration . unGDP2
    {-# INLINE gdSlotDuration #-}
    gdMaxBlockEnergy = case protocolVersion @pv of
        SP1 -> gdMaxBlockEnergy . unGDP1
        SP2 -> gdMaxBlockEnergy . unGDP2
    {-# INLINE gdMaxBlockEnergy #-}
    gdFinalizationParameters = case protocolVersion @pv of
        SP1 -> gdFinalizationParameters . unGDP1
        SP2 -> gdFinalizationParameters . unGDP2
    {-# INLINE gdFinalizationParameters #-}
    gdEpochLength = case protocolVersion @pv of
        SP1 -> gdEpochLength . unGDP1
        SP2 -> gdEpochLength . unGDP2
    {-# INLINE gdEpochLength #-}

instance (IsProtocolVersion pv) => Eq (GenesisData pv) where
    (==) = case protocolVersion @pv of
        SP1 -> (==) `on` unGDP1
        SP2 -> (==) `on` unGDP2

instance (IsProtocolVersion pv) => Serialize (GenesisData pv) where
    get = case protocolVersion @pv of
        SP1 -> GDP1 <$> P1.getGenesisDataV3
        SP2 -> GDP2 <$> P2.getGenesisDataV4
    put = case protocolVersion @pv of
        SP1 -> P1.putGenesisDataV3 . unGDP1
        SP2 -> P2.putGenesisDataV4 . unGDP2

-- |Deserialize genesis data with a version tag.
-- See `putVersionedGenesisData` for details of the version tag.
getVersionedGenesisData :: forall pv. IsProtocolVersion pv => Get (GenesisData pv)
getVersionedGenesisData = case protocolVersion @pv of
    SP1 -> GDP1 <$> P1.getVersionedGenesisData
    SP2 -> GDP2 <$> P2.getVersionedGenesisData

-- |Serialize genesis data with a version tag.
-- Each version tag must be specific to a protocol version, though more than one version tag can
-- be used for the same protocol version.
-- The currently supported versions are:
--
-- +-------------+------------------+
-- | Version tag | Protocol version |
-- +=============+==================+
-- | 3           | P1               |
-- | 4           | P2               |
-- +-------------+------------------+
putVersionedGenesisData :: forall pv. IsProtocolVersion pv => Putter (GenesisData pv)
putVersionedGenesisData = case protocolVersion @pv of
    SP1 -> P1.putVersionedGenesisData . unGDP1
    SP2 -> P2.putVersionedGenesisData . unGDP2

-- |Generate the block hash of a genesis block with the given genesis data.
-- This is based on the presumption that a block hash is computed from a byte string
-- beginning with the serialization of the block slot.
genesisBlockHash :: forall pv. IsProtocolVersion pv => GenesisData pv -> BlockHash
genesisBlockHash = case protocolVersion @pv of
    SP1 -> P1.genesisBlockHash . unGDP1
    SP2 -> P2.genesisBlockHash . unGDP2

-- |A dependent pair of a protocol version and genesis data.
data PVGenesisData = forall pv. IsProtocolVersion pv => PVGenesisData (GenesisData pv)

-- |Deserialize genesis data with a version tag to a 'PVGenesisData'.
-- This should attempt to parse with all supported protocol versions.
-- The version tag will uniquely determine the protocol version.
-- For details, see `putVersionedGenesisData`.
getPVGenesisData :: Get PVGenesisData
getPVGenesisData = do
  getVersion >>= \case
    3 -> PVGenesisData . GDP1 <$> P1.getGenesisDataV3
    4 -> PVGenesisData . GDP2 <$> P2.getGenesisDataV4
    n -> fail $ "Unsupported genesis version: " ++ show n
