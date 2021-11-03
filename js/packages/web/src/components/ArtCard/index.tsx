import React, { useEffect, useMemo, useState } from 'react';
import { Card, CardProps, Button, Badge } from 'antd';
import { MetadataCategory } from '@oyster/common';
import { ArtContent } from './../ArtContent';
import './index.less';
import { useArt } from '../../hooks';
import { PublicKey } from '@solana/web3.js';
import { Artist, ArtType } from '../../types';
import { MetaAvatar } from '../MetaAvatar';

const { Meta } = Card;

export interface ArtCardProps extends CardProps {
  pubkey?: PublicKey;
  image?: string;
  file?: string;
  blob?: Blob;
  category?: MetadataCategory;
  name?: string;
  symbol?: string;
  description?: string;
  creators?: Artist[];
  preview?: boolean;
  small?: boolean;
  close?: () => void;
  endAuctionAt?: number;
}

export const ArtCard = (props: ArtCardProps) => {
  let {
    className,
    small,
    category,
    image,
    file,
    name,
    preview,
    creators,
    description,
    close,
    pubkey,
    ...rest
  } = props;
  const art = useArt(pubkey);
  category = art?.category || category;
  image = art?.image || image;
  creators = art?.creators || creators || [];
  name = art?.title || name || ' ';
  description = art?.about || description;

  let badge = '';
  if (art.type === ArtType.NFT) {
    badge = 'Unique';
  } else if (art.type === ArtType.Master) {
    badge = 'NFT 0';
  } else if (art.type === ArtType.Print) {
    badge = `${art.edition} of ${art.supply}`;
  }

  const card = (
    <Card
      hoverable={true}
      className={`art-card ${small ? 'small' : ''} ${className ?? ''}`}
      cover={
        <>
          {close && (
            <Button
              className="card-close-button"
              shape="circle"
              onClick={e => {
                e.stopPropagation();
                e.preventDefault();
                close && close();
              }}
            >
              X
            </Button>
          )}
          <ArtContent
            category={category}
            extension={file || image}
            files={art.files}
            uri={image}
            preview={preview}
          />
        </>
      }
      {...rest}
    >
      <Meta
        title={`${name}`}
        description={
          <>
            <MetaAvatar creators={creators} size={32} />
            {/* {art.type === ArtType.Master && (
              <>
                <br />
                {!endAuctionAt && (
                  <span style={{ padding: '24px' }}>
                    {(art.maxSupply || 0) - (art.supply || 0)}/
                    {art.maxSupply || 0} prints remaining
                  </span>
                )}
              </>
            )} */}
            <div className="edition-badge">{badge}</div>
          </>
        }
      />
    </Card>
  );

  return art.creators?.find(c => !c.verified) ? (
    <Badge.Ribbon text="Unverified">{card}</Badge.Ribbon>
  ) : (
    card
  );
};
