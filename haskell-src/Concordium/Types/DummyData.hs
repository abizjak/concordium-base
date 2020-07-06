{-# OPTIONS_GHC -Wno-deprecations #-}
module Concordium.Types.DummyData where

import qualified Concordium.Crypto.SignatureScheme as SigScheme
import Concordium.Types
import qualified Data.PQueue.Prio.Max as Queue
import Concordium.Crypto.DummyData
import Concordium.ID.DummyData
import Concordium.ID.Types
import System.Random
import Concordium.Crypto.SHA256
import Data.FixedByteString as FBS
import Lens.Micro.Platform
import Concordium.Crypto.VRF as VRF

-- This generates an account with a single credential, the given list of keys and signature threshold,
-- which has sufficiently late expiry date, but is otherwise not well-formed.
-- The keys are indexed in ascending order starting from 0
{-# WARNING mkAccountMultipleKeys "Do not use in production." #-}
mkAccountMultipleKeys :: [SigScheme.VerifyKey] -> SignatureThreshold -> AccountAddress -> Amount -> Account
mkAccountMultipleKeys keys threshold addr amount = mkAccountNoCredentials keys threshold addr amount
      & (accountCredentials .~ (Queue.singleton dummyMaxValidTo (dummyCredential addr dummyMaxValidTo dummyCreatedAt)))

-- This generates an account without any credentials
-- late expiry date, but is otherwise not well-formed.
{-# WARNING mkAccountNoCredentials "Do not use in production." #-}
mkAccountNoCredentials :: [SigScheme.VerifyKey] -> SignatureThreshold -> AccountAddress -> Amount -> Account
mkAccountNoCredentials keys threshold addr amnt =
  newAccount (makeAccountKeys keys threshold) addr (dummyRegId addr) & (accountAmount .~ amnt)

-- This generates an account with a single credential and single keypair, which has sufficiently
-- late expiry date, but is otherwise not well-formed.
{-# WARNING mkAccount "Do not use in production." #-}
mkAccount :: SigScheme.VerifyKey -> AccountAddress -> Amount -> Account
mkAccount key addr amnt = mkAccountMultipleKeys [key] 1 addr amnt
      & (accountCredentials .~ (Queue.singleton dummyMaxValidTo (dummyCredential addr dummyMaxValidTo dummyCreatedAt)))

{-# WARNING makeFakeBakerAccount "Do not use in production." #-}
makeFakeBakerAccount :: BakerId -> Account
makeFakeBakerAccount bid =
    acct {_accountAmount = 1000000000000,
          _accountStakeDelegate = Just bid,
          _accountCredentials = credentialList}
  where
    vfKey = SigScheme.correspondingVerifyKey kp
    credential = dummyCredential address dummyMaxValidTo dummyCreatedAt
    credentialList = Queue.singleton dummyMaxValidTo credential
    acct = newAccount (makeSingletonAC vfKey) address (cdvRegId credential)
    -- NB the negation makes it not conflict with other fake accounts we create elsewhere.
    seed = - (fromIntegral bid) - 1
    (address, seed') = randomAccountAddress (mkStdGen seed)
    kp = uncurry SigScheme.KeyPairEd25519 $ fst (randomEd25519KeyPair seed')

{-# WARNING dummyblockPointer "Do not use in production." #-}
dummyblockPointer :: BlockHash
dummyblockPointer = Hash (FBS.pack (replicate 32 (fromIntegral (0 :: Word))))

{-# WARNING mateuszAccount "Do not use in production." #-}
mateuszAccount :: AccountAddress
mateuszAccount = accountAddressFrom 0

{-# WARNING alesAccount "Do not use in production." #-}
alesAccount :: AccountAddress
alesAccount = accountAddressFrom 1

{-# WARNING thomasAccount "Do not use in production." #-}
thomasAccount :: AccountAddress
thomasAccount = accountAddressFrom 2

{-# WARNING accountAddressFrom "Do not use in production." #-}
accountAddressFrom :: Int -> AccountAddress
accountAddressFrom n = fst (randomAccountAddress (mkStdGen n))

{-# WARNING accountAddressFromCred "Do not use in production." #-}
accountAddressFromCred :: CredentialDeploymentInformation -> AccountAddress
accountAddressFromCred = credentialAccountAddress . cdiValues

-- The expiry time is set to the same time as slot time, which is currently also 0.
-- If slot time increases, in order for tests to pass transaction expiry must also increase.
{-# WARNING dummyLowTransactionExpiryTime "Do not use in production." #-}
dummyLowTransactionExpiryTime :: TransactionExpiryTime
dummyLowTransactionExpiryTime = 0

{-# WARNING dummyMaxTransactionExpiryTime "Do not use in production." #-}
dummyMaxTransactionExpiryTime :: TransactionExpiryTime
dummyMaxTransactionExpiryTime = TransactionExpiryTime maxBound

{-# WARNING dummySlotTime "Do not use in production." #-}
dummySlotTime :: Timestamp
dummySlotTime = 0

{-# WARNING bakerElectionKey "Do not use in production." #-}
bakerElectionKey :: Int -> BakerElectionPrivateKey
bakerElectionKey n = fst (VRF.randomKeyPair (mkStdGen n))

{-# WARNING bakerSignKey "Do not use in production." #-}
bakerSignKey :: Int -> BakerSignPrivateKey
bakerSignKey n = fst (randomBlockKeyPair (mkStdGen n))

{-# WARNING bakerAggregationKey "Do not use in production." #-}
bakerAggregationKey :: Int -> BakerAggregationPrivateKey
bakerAggregationKey n = fst (randomBlsSecretKey (mkStdGen n))
