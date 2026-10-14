import { schemaValidator } from '@forklaunch/blueprint-core';
import { requestMapper, responseMapper } from '@forklaunch/core/mappers';
import { EntityManager } from '@mikro-orm/core';
import Stripe from 'stripe';
import { Subscription } from '../../persistence/entities/subscription.entity';
import { PartyEnum } from '../enum/party.enum';
import { SubscriptionSchemas } from '../schemas';

const CreateSubscriptionEntityMapper = requestMapper({
  schemaValidator,
  schema: SubscriptionSchemas.CreateSubscriptionSchema(PartyEnum),
  entity: Subscription,
  mapperDefinition: {
    toEntity: async (
      dto,
      em: EntityManager,
      providerFields: Stripe.Subscription
    ) => {
      return em.create(Subscription, {
        partyId: dto.partyId,
        partyType: dto.partyType,
        productId: dto.productId,
        description: dto.description ?? null,
        active: dto.active,
        externalId: dto.externalId,
        startDate: dto.startDate,
        endDate: dto.endDate ?? null,
        status: dto.status,
        billingProvider: dto.billingProvider,
        providerFields
      });
    }
  }
});

const UpdateSubscriptionEntityMapper = requestMapper({
  schemaValidator,
  schema: SubscriptionSchemas.UpdateSubscriptionSchema(PartyEnum),
  entity: Subscription,
  mapperDefinition: {
    toEntity: async (
      dto,
      em: EntityManager,
      providerFields: Stripe.Subscription
    ) => {
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      const { stripeFields, ...rest } = dto;
      const entity = await em.findOneOrFail(Subscription, { id: rest.id });
      em.assign(entity, {
        ...rest,
        providerFields
      });
      return entity;
    }
  }
});

const SubscriptionEntityMapper = responseMapper({
  schemaValidator,
  schema: SubscriptionSchemas.SubscriptionSchema(PartyEnum),
  entity: Subscription,
  mapperDefinition: {
    toDto: async (entity) => {
      return {
        ...entity,
        description: entity.description ?? undefined,
        endDate: entity.endDate ?? undefined,
        billingProvider: entity.billingProvider ?? undefined,
        stripeFields: entity.providerFields
      };
    }
  }
});

export const SubscriptionMapper = {
  ...SubscriptionEntityMapper,
  partyTypes: { user: PartyEnum.USER, organization: PartyEnum.ORGANIZATION }
};

export const UpdateSubscriptionMapper = {
  ...UpdateSubscriptionEntityMapper,
  validateUpdate: async (
    dto: Parameters<typeof UpdateSubscriptionEntityMapper.toEntity>[0],
    existing: Awaited<ReturnType<typeof SubscriptionEntityMapper.toDto>>
  ) => {
    if (
      typeof existing.id !== 'string' ||
      !existing.id.trim() ||
      existing.id !== dto.id ||
      typeof existing.partyId !== 'string' ||
      !existing.partyId.trim() ||
      (dto.partyId !== undefined && dto.partyId !== existing.partyId) ||
      (dto.partyType !== undefined && dto.partyType !== existing.partyType)
    ) {
      throw new Error('Subscription ownership cannot change during an update');
    }
    const patch = schemaValidator.parse(
      schemaValidator.schemify(
        SubscriptionSchemas.UpdateSubscriptionSchema(PartyEnum)
      ),
      dto
    );
    if (!patch.ok) {
      throw new Error('Invalid subscription update');
    }
  }
};

export const CreateSubscriptionMapper = {
  ...CreateSubscriptionEntityMapper,
  validateCreate: async (
    dto: Parameters<typeof CreateSubscriptionEntityMapper.toEntity>[0]
  ) => {
    // The service replaces these fields with provider or server-generated values.
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const { externalId, billingProvider, ...inputSchema } =
      SubscriptionSchemas.CreateSubscriptionSchema(PartyEnum);
    const input = schemaValidator.parse(
      schemaValidator.schemify(inputSchema),
      dto
    );
    if (!input.ok) {
      throw new Error('Invalid subscription creation');
    }
  }
};
