use {
    crate::{AuctionHouse, ErrorCode},
    anchor_lang::{
        prelude::*,
        solana_program::{
            program::{invoke, invoke_signed},
            program_option::COption,
            program_pack::{IsInitialized, Pack},
            system_instruction,
        },
    },
    anchor_spl::token::{Mint, TokenAccount},
    metaplex_token_metadata::state::Metadata,
    spl_associated_token_account::get_associated_token_address,
    spl_token::{instruction::initialize_account2, state::Account},
    std::{convert::TryInto, slice::Iter},
};
pub fn assert_is_ata(
    ata: &AccountInfo,
    wallet: &Pubkey,
    mint: &Pubkey,
) -> Result<Account, ProgramError> {
    assert_owned_by(ata, &spl_token::id())?;
    let ata_account: Account = assert_initialized(ata)?;
    assert_keys_equal(ata_account.owner, *wallet)?;
    assert_keys_equal(get_associated_token_address(wallet, mint), *ata.key)?;
    Ok(ata_account)
}

pub fn make_ata<'a>(
    ata: AccountInfo<'a>,
    wallet: AccountInfo<'a>,
    mint: AccountInfo<'a>,
    fee_payer: AccountInfo<'a>,
    ata_program: AccountInfo<'a>,
    token_program: AccountInfo<'a>,
    system_program: AccountInfo<'a>,
    rent: AccountInfo<'a>,
    fee_payer_seeds: &[&[u8]],
) -> ProgramResult {
    invoke_signed(
        &spl_associated_token_account::create_associated_token_account(
            &fee_payer.key,
            &wallet.key,
            &mint.key,
        ),
        &[
            ata,
            wallet,
            mint,
            fee_payer,
            ata_program,
            system_program,
            rent,
            token_program,
        ],
        &[fee_payer_seeds],
    )?;

    Ok(())
}

pub fn assert_metadata_valid<'a>(
    metadata: &UncheckedAccount,
    token_account: &anchor_lang::Account<'a, TokenAccount>,
) -> ProgramResult {
    assert_derivation(
        &metaplex_token_metadata::id(),
        &metadata.to_account_info(),
        &[
            metaplex_token_metadata::state::PREFIX.as_bytes(),
            metaplex_token_metadata::id().as_ref(),
            token_account.mint.as_ref(),
        ],
    )?;

    if metadata.data_is_empty() {
        return Err(ErrorCode::MetadataDoesntExist.into());
    }
    Ok(())
}

pub fn get_fee_payer<'a, 'b>(
    authority: &UncheckedAccount,
    auction_house: &anchor_lang::Account<AuctionHouse>,
    wallet: AccountInfo<'a>,
    auction_house_fee_account: AccountInfo<'a>,
    auction_house_seeds: &'b [&'b [u8]],
) -> Result<(AccountInfo<'a>, &'b [&'b [u8]]), ProgramError> {
    let mut seeds: &[&[u8]] = &[];
    let fee_payer: AccountInfo;
    if authority.to_account_info().is_signer {
        seeds = auction_house_seeds;
        fee_payer = auction_house_fee_account;
    } else if wallet.is_signer {
        if auction_house.requires_sign_off {
            return Err(ErrorCode::CannotTakeThisActionWithoutAuctionHouseSignOff.into());
        }
        fee_payer = wallet
    } else {
        return Err(ErrorCode::NoPayerPresent.into());
    };

    Ok((fee_payer, &seeds))
}

pub fn assert_valid_delegation(
    src_account: &AccountInfo,
    dst_account: &AccountInfo,
    src_wallet: &AccountInfo,
    dst_wallet: &AccountInfo,
    transfer_authority: &AccountInfo,
    mint: &anchor_lang::Account<Mint>,
    paysize: u64,
) -> ProgramResult {
    match Account::unpack(&src_account.data.borrow()) {
        Ok(token_account) => {
            // Ensure that the delegated amount is exactly equal to the maker_size
            msg!(
                "Delegate {}",
                token_account.delegate.unwrap_or(*src_wallet.key)
            );
            msg!("Delegated Amount {}", token_account.delegated_amount);
            if token_account.delegated_amount != paysize {
                return Err(ProgramError::InvalidAccountData);
            }
            // Ensure that authority is the delegate of this token account
            msg!("Authority key matches");
            if token_account.delegate != COption::Some(*transfer_authority.key) {
                return Err(ProgramError::InvalidAccountData);
            }

            msg!("Delegate matches");
            assert_is_ata(src_account, src_wallet.key, &mint.key())?;
            assert_is_ata(dst_account, dst_wallet.key, &mint.key())?;
            msg!("ATAs match")
        }
        Err(_) => {
            if mint.key() != spl_token::native_mint::id() {
                return Err(ErrorCode::ExpectedSolAccount.into());
            }

            if !src_wallet.is_signer {
                return Err(ErrorCode::SOLWalletMustSign.into());
            }

            assert_keys_equal(*src_wallet.key, src_account.key())?;
            assert_keys_equal(*dst_wallet.key, dst_account.key())?;
        }
    }

    Ok(())
}

pub fn assert_keys_equal(key1: Pubkey, key2: Pubkey) -> ProgramResult {
    if key1 != key2 {
        Err(ErrorCode::PublicKeyMismatch.into())
    } else {
        Ok(())
    }
}

pub fn assert_initialized<T: Pack + IsInitialized>(
    account_info: &AccountInfo,
) -> Result<T, ProgramError> {
    let account: T = T::unpack_unchecked(&account_info.data.borrow())?;
    if !account.is_initialized() {
        Err(ErrorCode::UninitializedAccount.into())
    } else {
        Ok(account)
    }
}

pub fn assert_owned_by(account: &AccountInfo, owner: &Pubkey) -> ProgramResult {
    if account.owner != owner {
        Err(ErrorCode::IncorrectOwner.into())
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub fn pay_auction_house_fees<'a>(
    auction_house: &anchor_lang::Account<'a, AuctionHouse>,
    auction_house_treasury: &AccountInfo<'a>,
    escrow_payment_account: &AccountInfo<'a>,
    token_program: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    escrow_signer_seeds: &[&[u8]],
    size: u64,
    is_native: bool,
) -> Result<u64, ProgramError> {
    let fees = auction_house.seller_fee_basis_points;
    let total_fee = (fees as u128)
        .checked_mul(size as u128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as u64;
    if !is_native {
        invoke_signed(
            &spl_token::instruction::transfer(
                token_program.key,
                &escrow_payment_account.key,
                &auction_house_treasury.key,
                escrow_payment_account.key,
                &[],
                total_fee,
            )?,
            &[
                escrow_payment_account.clone(),
                auction_house_treasury.clone(),
                token_program.clone(),
            ],
            &[escrow_signer_seeds],
        )?;
    } else {
        invoke_signed(
            &system_instruction::transfer(
                &escrow_payment_account.key,
                auction_house_treasury.key,
                total_fee,
            ),
            &[
                escrow_payment_account.clone(),
                auction_house_treasury.clone(),
                system_program.clone(),
            ],
            &[escrow_signer_seeds],
        )?;
    }
    Ok(total_fee)
}

pub fn create_escrow_if_not_present<'a>(
    escrow_payment_account: &UncheckedAccount<'a>,
    system_program: &UncheckedAccount<'a>,
    fee_payer: &AccountInfo<'a>,
    token_program: &UncheckedAccount<'a>,
    treasury_mint: &anchor_lang::Account<'a, Mint>,
    rent: &Sysvar<'a, Rent>,
    program_id: &Pubkey,
    escrow_signer_seeds: &[&[u8]],
    fee_seeds: &[&[u8]],
    is_native: bool,
) -> ProgramResult {
    if !is_native && escrow_payment_account.data_is_empty() {
        create_or_allocate_account_raw(
            *program_id,
            &escrow_payment_account.to_account_info(),
            &rent.to_account_info(),
            &system_program,
            &fee_payer,
            spl_token::state::Account::LEN,
            fee_seeds,
        )?;

        invoke_signed(
            &initialize_account2(
                &token_program.key,
                &escrow_payment_account.key(),
                &treasury_mint.key(),
                &escrow_payment_account.key(),
            )
            .unwrap(),
            &[
                token_program.to_account_info(),
                treasury_mint.to_account_info(),
                escrow_payment_account.to_account_info(),
                rent.to_account_info(),
            ],
            &[&escrow_signer_seeds],
        )?;
    } else if is_native && escrow_payment_account.owner != program_id {
        invoke_signed(
            &system_instruction::assign(&escrow_payment_account.key(), program_id),
            &[
                system_program.to_account_info(),
                escrow_payment_account.to_account_info(),
            ],
            &[&escrow_signer_seeds],
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn pay_creator_fees<'a>(
    remaining_accounts: &mut Iter<AccountInfo<'a>>,
    metadata_info: &AccountInfo<'a>,
    escrow_payment_account: &AccountInfo<'a>,
    fee_payer: &AccountInfo<'a>,
    treasury_mint: &anchor_lang::Account<'a, Mint>,
    ata_program: &AccountInfo<'a>,
    token_program: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    rent: &AccountInfo<'a>,
    escrow_signer_seeds: &[&[u8]],
    fee_payer_seeds: &[&[u8]],
    size: u64,
    is_native: bool,
) -> Result<u64, ProgramError> {
    let metadata = Metadata::from_account_info(metadata_info)?;
    let fees = metadata.data.seller_fee_basis_points;
    let total_fee = (fees as u128)
        .checked_mul(size as u128)
        .ok_or(ErrorCode::NumericalOverflow)?
        .checked_div(10000)
        .ok_or(ErrorCode::NumericalOverflow)? as u64;
    let mut remaining_fee = total_fee;
    let remaining_size = size
        .checked_sub(total_fee)
        .ok_or(ErrorCode::NumericalOverflow)?;
    match metadata.data.creators {
        Some(creators) => {
            for creator in creators {
                let pct = creator.share as u128;
                let creator_fee = pct
                    .checked_mul(total_fee as u128)
                    .ok_or(ErrorCode::NumericalOverflow)?
                    .checked_div(100)
                    .ok_or(ErrorCode::NumericalOverflow)? as u64;
                remaining_fee = remaining_fee
                    .checked_sub(creator_fee)
                    .ok_or(ErrorCode::NumericalOverflow)?;
                let current_creator_info = next_account_info(remaining_accounts)?;
                assert_keys_equal(creator.address, *current_creator_info.key)?;
                if !is_native {
                    let current_creator_token_account_info = next_account_info(remaining_accounts)?;
                    if current_creator_token_account_info.data_is_empty() {
                        make_ata(
                            current_creator_token_account_info.to_account_info(),
                            current_creator_info.to_account_info(),
                            treasury_mint.to_account_info(),
                            fee_payer.to_account_info(),
                            ata_program.to_account_info(),
                            token_program.to_account_info(),
                            system_program.to_account_info(),
                            rent.to_account_info(),
                            fee_payer_seeds,
                        )?;
                    }
                    assert_is_ata(
                        current_creator_token_account_info,
                        current_creator_info.key,
                        &treasury_mint.key(),
                    )?;
                    if creator_fee > 0 {
                        invoke_signed(
                            &spl_token::instruction::transfer(
                                token_program.key,
                                &escrow_payment_account.key,
                                current_creator_token_account_info.key,
                                escrow_payment_account.key,
                                &[],
                                creator_fee,
                            )?,
                            &[
                                escrow_payment_account.clone(),
                                current_creator_token_account_info.clone(),
                                token_program.clone(),
                            ],
                            &[escrow_signer_seeds],
                        )?;
                    }
                } else if creator_fee > 0 {
                    invoke_signed(
                        &system_instruction::transfer(
                            &escrow_payment_account.key,
                            current_creator_info.key,
                            creator_fee,
                        ),
                        &[
                            escrow_payment_account.clone(),
                            current_creator_info.clone(),
                            system_program.clone(),
                        ],
                        &[escrow_signer_seeds],
                    )?;
                }
            }
        }
        None => {
            msg!("No creators found in metadata");
        }
    }
    // Any dust is returned to the party posting the NFT
    Ok(remaining_size
        .checked_add(remaining_fee)
        .ok_or(ErrorCode::NumericalOverflow)?)
}

/// Create account almost from scratch, lifted from
/// https://github.com/solana-labs/solana-program-library/blob/7d4873c61721aca25464d42cc5ef651a7923ca79/associated-token-account/program/src/processor.rs#L51-L98
#[inline(always)]
pub fn create_or_allocate_account_raw<'a>(
    program_id: Pubkey,
    new_account_info: &AccountInfo<'a>,
    rent_sysvar_info: &AccountInfo<'a>,
    system_program_info: &AccountInfo<'a>,
    payer_info: &AccountInfo<'a>,
    size: usize,
    signer_seeds: &[&[u8]],
) -> Result<(), ProgramError> {
    let rent = &Rent::from_account_info(rent_sysvar_info)?;
    let required_lamports = rent
        .minimum_balance(size)
        .max(1)
        .saturating_sub(new_account_info.lamports());

    if required_lamports > 0 {
        msg!("Transfer {} lamports to the new account", required_lamports);
        invoke(
            &system_instruction::transfer(&payer_info.key, new_account_info.key, required_lamports),
            &[
                payer_info.clone(),
                new_account_info.clone(),
                system_program_info.clone(),
            ],
        )?;
    }

    let accounts = &[new_account_info.clone(), system_program_info.clone()];

    msg!("Allocate space for the account");
    invoke_signed(
        &system_instruction::allocate(new_account_info.key, size.try_into().unwrap()),
        accounts,
        &[&signer_seeds],
    )?;

    msg!("Assign the account to the owning program");
    invoke_signed(
        &system_instruction::assign(new_account_info.key, &program_id),
        accounts,
        &[&signer_seeds],
    )?;
    msg!("Completed assignation!");

    Ok(())
}

pub fn assert_derivation(
    program_id: &Pubkey,
    account: &AccountInfo,
    path: &[&[u8]],
) -> Result<u8, ProgramError> {
    let (key, bump) = Pubkey::find_program_address(&path, program_id);
    if key != *account.key {
        return Err(ErrorCode::DerivedKeyInvalid.into());
    }
    Ok(bump)
}
