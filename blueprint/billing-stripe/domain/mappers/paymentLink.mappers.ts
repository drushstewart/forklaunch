import { schemaValidator } from '@forklaunch/blueprint-core';
import { requestMapper, responseMapper } from '@forklaunch/core/mappers';
import { EntityManager } from '@mikro-orm/core';
import Stripe from 'stripe';
import { PaymentLink } from '../../persistence/entities/paymentLink.entity';
import { StatusEnum } from '../enum/status.enum';
import { PaymentLinkSchemas } from '../schemas';

const CreatePaymentLinkEntityMapper = requestMapper({
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

const UpdatePaymentLinkEntityMapper = requestMapper({
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

function cachedPaymentLinkEvidence(
  existing: Awaited<ReturnType<typeof PaymentLinkEntityMapper.toDto>>
) {
  const parsed = schemaValidator.parse(
    schemaValidator.schemify(PaymentLinkSchemas.PaymentLinkSchema(StatusEnum)),
    existing
  );
  if (!parsed.ok || !parsed.value.id.trim()) {
    throw new Error('Invalid cached payment link response');
  }
  return paymentLinkEvidence(parsed.value.stripeFields);
}

export const PaymentLinkMapper = {
  ...PaymentLinkEntityMapper,
  normalizeCachedDto: (
    value: Awaited<ReturnType<typeof PaymentLinkEntityMapper.toDto>>
  ) => {
    const parsed = schemaValidator.parse(
      schemaValidator.schemify(PaymentLinkSchemas.PaymentLinkSchema(StatusEnum)),
      value
    );
    if (!parsed.ok) {
      throw new Error('Invalid cached payment link response');
    }
    return { ...value, ...parsed.value };
  },
  validateCachedRead: (
    existing: Awaited<ReturnType<typeof PaymentLinkEntityMapper.toDto>>
  ) => {
    cachedPaymentLinkEvidence(existing);
  },
  readCachedDto: async (
    existing: Awaited<ReturnType<typeof PaymentLinkEntityMapper.toDto>>,
    retrieve: (providerId: string) => Promise<Stripe.PaymentLink>
  ) => {
    const before = cachedPaymentLinkEvidence(existing);
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

export const UpdatePaymentLinkMapper = {
  ...UpdatePaymentLinkEntityMapper,
  validateCachedUpdate: async (
    dto: Parameters<typeof UpdatePaymentLinkEntityMapper.toEntity>[0],
    existing: Awaited<ReturnType<typeof PaymentLinkEntityMapper.toDto>>
  ) => {
    // The base read has already normalized the configured response DTO once.
    // Validate the patch without mutating either input or using cached provider
    // references as mutation authority. Provider target dispatch is unchanged.
    if (
      typeof existing.id !== 'string' ||
      !existing.id.trim() ||
      existing.id !== dto.id
    ) {
      throw new Error('Cached payment link identity cannot change');
    }
    const patch = schemaValidator.parse(
      schemaValidator.schemify(
        PaymentLinkSchemas.UpdatePaymentLinkSchema(StatusEnum)
      ),
      dto
    );
    if (!patch.ok) {
      throw new Error('Invalid cached payment link update');
    }
  }
};

export const CreatePaymentLinkMapper = {
  ...CreatePaymentLinkEntityMapper,
  validateCreate: async (
    dto: Parameters<typeof CreatePaymentLinkEntityMapper.toEntity>[0]
  ) => {
    // The service derives a nullish amount and replaces the id from Stripe.
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const { id, amount, ...inputSchema } =
      PaymentLinkSchemas.CreatePaymentLinkSchema(StatusEnum);
    const input = schemaValidator.parse(
      schemaValidator.schemify({ ...inputSchema, ...(dto.amount == null ? {} : { amount }) }),
      dto
    );
    if (!input.ok) {
      throw new Error('Invalid paymentLink creation');
    }
  }
};
