import { EntityManager, InferEntity } from '@mikro-orm/core';
import Stripe from 'stripe';
import { StripeBillingPortalDtos } from './stripe.dto.types';
import { StripeBillingPortalEntities } from './stripe.entity.types';

export type StripeBillingPortalMappers<
  Entities extends StripeBillingPortalEntities,
  Dto extends StripeBillingPortalDtos
> = {
  BillingPortalMapper: {
    entity: Entities['BillingPortalMapper'];
    toDto: (
      entity: InferEntity<Entities['BillingPortalMapper']>
    ) => Promise<Dto['BillingPortalMapper']>;
  };
  CreateBillingPortalMapper: {
    entity: Entities['CreateBillingPortalMapper'];
    toEntity: (
      dto: Dto['CreateBillingPortalMapper'],
      em: EntityManager,
      stripeSession: Stripe.BillingPortal.Session,
      ...args: unknown[]
    ) => Promise<InferEntity<Entities['CreateBillingPortalMapper']>>;
  };
  UpdateBillingPortalMapper: {
    entity: Entities['UpdateBillingPortalMapper'];
    /** Cache-only DTO update; preserve session/customer identities. */
    toCachedDto?: (
      dto: Dto['UpdateBillingPortalMapper'],
      existing: Dto['BillingPortalMapper'],
      em: EntityManager,
      stripeSession: Stripe.BillingPortal.Session,
      ...args: unknown[]
    ) => Promise<Dto['BillingPortalMapper']>;
    toEntity: (
      dto: Dto['UpdateBillingPortalMapper'],
      em: EntityManager,
      stripeSession: Stripe.BillingPortal.Session,
      ...args: unknown[]
    ) => Promise<InferEntity<Entities['UpdateBillingPortalMapper']>>;
  };
};
