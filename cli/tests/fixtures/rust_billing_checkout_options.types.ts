// Compile with the actual billing DTO source and Stripe SDK types.
import type {
  StripeCheckoutSessionDto,
  StripeCreateCheckoutSessionDto,
  StripeUpdateCheckoutSessionDto
} from '../../../blueprint/implementations/billing/stripe/domain/types/stripe.dto.types';

type Assert<T extends true> = T;
type Optional<T, K extends keyof T> = {} extends Pick<T, K> ? true : false;
export type CreateOptionsCanBeOmitted = Assert<
  Optional<StripeCreateCheckoutSessionDto<'PENDING'>, 'stripeFields'>
>;
export type UpdateOptionsStayOptional = Assert<
  Optional<StripeUpdateCheckoutSessionDto<'PENDING'>, 'stripeFields'>
>;
export type ResponseEvidenceStaysRequired = Assert<
  Optional<StripeCheckoutSessionDto<'PENDING'>, 'stripeFields'> extends false
    ? true
    : false
>;
