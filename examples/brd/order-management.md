# Order Management – Business Requirements (DEMONSTRATION BRD)

This document is a labelled demonstration fixture for EAEH Phase 3. It describes a
fictional retailer and is not a real business requirements document.

## Business context

The retailer sells products online. Customers currently phone in orders, which causes
errors and delays. The business wants customers to place and track orders themselves.

## Placing orders

A registered customer places an order for one or more products from their cart. An order
must contain at least one line, and each line has a product and a positive quantity. When
an order is placed the customer receives a confirmation with an order number.

## Cancelling orders

A customer can cancel an order until it has been shipped. Cancellation rules for partially
shipped orders are TBD.

## Order status

Customers can see the current status of each of their orders: placed, paid, shipped,
delivered or cancelled. Warehouse staff update the status when an order is shipped.

## Payments

Payment is taken by the existing external payment provider when the order is placed. The
platform never stores card numbers.

## Performance and availability

Order placement must be fast. The service must be available 99.9% of each calendar month.

## Out of scope

Returns and refunds are handled by the existing customer-service process and are not part
of this initiative.
