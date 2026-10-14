import { EntityManager, InferEntity } from '@mikro-orm/core';
import Stripe from 'stripe';
import { StripeSubscriptionDtos } from './stripe.dto.types';
import { StripeSubscriptionEntities } from './stripe.entity.types';

export type StripeSubscriptionMappers<
  PartyTypeEnum,
  Entities extends StripeSubscriptionEntities,
  Dto extends StripeSubscriptionDtos<PartyTypeEnum>
> = {
  SubscriptionMapper: {
    /** Trusted persisted party discriminators for local subscription lookups. */
    partyTypes?: Readonly<{ user: string; organization: string }>;
    entity: Entities['SubscriptionMapper'];
    toDto: (
      entity: InferEntity<Entities['SubscriptionMapper']>
    ) => Promise<Dto['SubscriptionMapper']>;
  };
  CreateSubscriptionMapper: {
    entity: Entities['CreateSubscriptionMapper'];
    toEntity: (
      dto: Dto['CreateSubscriptionMapper'],
      em: EntityManager,
      stripeSubscription: Stripe.Subscription,
      ...args: unknown[]
    ) => Promise<InferEntity<Entities['CreateSubscriptionMapper']>>;
  };
  UpdateSubscriptionMapper: {
    entity: Entities['UpdateSubscriptionMapper'];
    toEntity: (
      dto: Dto['UpdateSubscriptionMapper'],
      em: EntityManager,
      stripeSubscription: Stripe.Subscription,
      ...args: unknown[]
    ) => Promise<InferEntity<Entities['UpdateSubscriptionMapper']>>;
  };
};
