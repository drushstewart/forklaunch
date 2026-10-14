import { schemaValidator } from '@forklaunch/blueprint-core';
import { requestMapper, responseMapper } from '@forklaunch/core/mappers';
import { EntityManager } from '@mikro-orm/core';
import Stripe from 'stripe';
import { PaymentLink } from '../../persistence/entities/paymentLink.entity';
import { StatusEnum } from '../enum/status.enum';
import { PaymentLinkSchemas } from '../schemas';

export const CreatePaymentLinkMapper = requestMapper({
  schemaValidator,
  schema: PaymentLinkSchemas.CreatePaymentLinkSchema(StatusEnum),
  entity: PaymentLink,
  mapperDefinition: {
    toEntity: async (
      dto,
      em: EntityManager,
      providerFields: Stripe.PaymentLink
    ) => {
      return em.create(PaymentLink, {
        amount: dto.amount,
        paymentMethods: dto.paymentMethods,
        currency: dto.currency,
        description: null,
        status: dto.status,
        providerFields
      });
    }
  }
});

export const UpdatePaymentLinkMapper = requestMapper({
  schemaValidator,
  schema: PaymentLinkSchemas.UpdatePaymentLinkSchema(StatusEnum),
  entity: PaymentLink,
  mapperDefinition: {
    toEntity: async (
      dto,
      em: EntityManager,
      providerFields: Stripe.PaymentLink
    ) => {
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      const { stripeFields, ...rest } = dto;
      const entity = await em.findOneOrFail(PaymentLink, { id: rest.id });
      em.assign(entity, {
        ...rest,
        providerFields
      });
      return entity;
    }
  }
});

const PaymentLinkEntityMapper = responseMapper({
  schemaValidator,
  schema: PaymentLinkSchemas.PaymentLinkSchema(StatusEnum),
  entity: PaymentLink,
  mapperDefinition: {
    toDto: async (entity) => {
      return {
        ...entity,
        amount: Number(entity.amount),
        description: entity.description ?? undefined,
        stripeFields: entity.providerFields
      };
    }
  }
});

/** Read-only cached reference; this is not verified provider account ownership. */
function paymentLinkEvidence(value: unknown) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('Invalid payment link provider evidence');
  }
  const link = value as Record<string, unknown>;
  if (
    typeof link.id !== 'string' ||
    !/^plink_[A-Za-z0-9_]+$/.test(link.id) ||
    link.id.length > 128 ||
    link.object !== 'payment_link' ||
    typeof link.livemode !== 'boolean'
  ) {
    throw new Error('Invalid payment link provider evidence');
  }
  return { id: link.id, livemode: link.livemode };
}

export const PaymentLinkMapper = {
  ...PaymentLinkEntityMapper,
  readCachedDto: async (
    existing: Awaited<ReturnType<typeof PaymentLinkEntityMapper.toDto>>,
    retrieve: (providerId: string) => Promise<Stripe.PaymentLink>
  ) => {
    const parsed = schemaValidator.parse(
      schemaValidator.schemify(
        PaymentLinkSchemas.PaymentLinkSchema(StatusEnum)
      ),
      existing
    );
    if (!parsed.ok || !parsed.value.id.trim()) {
      throw new Error('Invalid cached payment link response');
    }
    const before = paymentLinkEvidence(parsed.value.stripeFields);
    const link = await retrieve(before.id);
    const after = paymentLinkEvidence(link);
    if (before.id !== after.id || before.livemode !== after.livemode) {
      throw new Error('Payment link provider evidence does not match');
    }
    // Preserve the public UUID and local fields; do not write provider evidence
    // into cache or use it to authorize mutations or managed account rebinding.
    return { ...existing, stripeFields: link };
  }
};
