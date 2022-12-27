# Implementation of A Simple Limit Order Book in Rust
A limit order match engine with RESTful API in Rust-lang.

This program is only for practice as an educational material. 
All data is stored in memory so that all data will be lost if restarting the server process.
This demo project only uses a trading pair DDX/USD. DDX represents a decentralized exchangeable derivative asset.

## REST API

The test harness expects an HTTP REST API conforming to the schema provided below to be exposed on port `4321`. This API should expose all of the functionality of our matching engine implementation and will be the interface that we'll use to run our test suite on the project.

### Data Structures

- Address
    - An ethereum addresses. Ethereum's addresses are 20 byte values. We expect them to be serialized as a hexadecimal number prefixed with `0x`.
    - Example: `0xb724D8C629A163d0E1809fAB27420fdf7f72a02c`
- Hash
    - A 32 byte number encoded as a hexadecimal number prefixed with `0x`.
    - Example: `0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470`
- Decimal
    - our matching engine should perform all mathematical operations with 18 decimals of precision. Whenever we see the `Decimal` type used below, we are referring to decimal numbers that have been serialized to strings with up to 18 decimals of precision.
    - Example: `1.30`
- Account:
    - A data structure containing a trader's ethereum address, and DDX & USD balances.
```
{
    ddxBalance: Decimal,
    usdBalance: Decimal,
    traderAddress: Address,
}
```
- Order:
    - A data structure representing a trader's desired intention to trade.
```
{
    amount: Decimal,
    nonce: Hash,
    price: Decimal,
    side: 'Bid' | 'Ask',
    traderAddress: Address,
}
```
- L2 order book:
    - A data structure representing an aggregate order book view. To be more explicit, the core matching engine implementation must maintain order-by-order granularity in order to perform specific matches, however this L2 aggregation is a convenient view by collapsing any given price level to the aggregate quantity at that level irrespective of the number of participants or the individual order details that comprise that price level.
```
{
    asks: [{
        amount: Decimal,
        price: Decimal,
    }],
    bids: [{
        amount: Decimal,
        price: Decimal,
    }],
}
```
- Fill:
    - A data structure that represents a fill that occurred by matching two orders.
```
{
    maker_hash: Hash,
    taker_hash: Hash,
    fill_amount: Decimal,
    price: Decimal,
}
```

#### EIP712 Hashing

In order to generate a probabilistically unique identifier to refer to each order, we will compute a EIP712 hash of the order. EIP712 is an Ethereum Improvement Proposal that specifies a hashing scheme that makes it possible to hash structured data and convey more information to users when they need to sign data for a blockchain interaction. The [specification](https://eips.ethereum.org/EIPS/eip-712) provides guidance on how to use this primitive. 
The production code of decentralized exchanges make extensive use of EIP712 hashing, so this will be a good opportunity for us to get comfortable with the primitive!

Collisions must be considered when designing any hashing scheme. Since traders may want to send duplicate orders with the same `amount`, `price`, and `side`, the hashing scheme include an additional field that will be unique across orders. The `nonce` field will be used to ensure that each order has a distinct hash. An order that includes the same `nonce` as another previously posted by the same trader is considered invalid and should be discarded.

##### EIP712 Domain Separator

We only include a name and a version in the EIP712 domain separator that we use. It is generally good practice utilizing the `chainId` and `verifyingContractAddress` fields of the EIP712 domain seperator, but these fields don't make sense in the context of our take-home assignment (our project doesn't use a blockchain or smart contract for settlement).

```
type EIP712DomainSeperator = {
    name: string,
    version: string,
}
```

##### Order Schema

The order should be encoded into a Solidity analogue of the JSON object defined above. The 18 decimal precision numbers should be converted into unsigned 256 bit numbers. The decimal number `1e-18` should be serialized as `1`. Serializing the `nonce` and `traderAddress` fields is more straightforward.

```
{
    amount: uint256,
    nonce: uint256,
    price: uint256,
    side: uint8,
    traderAddress: address,
}
```

A sample order we may use for testing is:

```
{
    amount: 1234, // uint256
    nonce: 12, // uint256
    price: 5432, // uint256
    side: 0, // uint8
    traderAddress: "0x3A880652F47bFaa771908C07Dd8673A787dAEd3A" // address
}
```

Be sure to use "Order" as the beginning of the string specifying the order schema hash types, something like this: "Order(uint256 amount,uint256 nonce,uint256 price,uint8 side,address traderAddress)"

A proper implementation for the above sample will result in an EIP-712 hash of: 0x15a7b83cc86b50aaa2fa0c0871d5dbaae62f116436291e976c84b034b58cb728

### Routes

- `/accounts`
    - `/`
        - `POST`: Create a new trader account
            - Body:
                - A JSON `Account` object
    - `/:traderAddress`
        - `GET`: Get an account by trader address
    - `/:traderAddress`
        - `DELETE`: Delete an account by trader address
- `/orders`
    - `/`
        - `POST`: Add an order to the orderbook (possibly matching other orders)
            - Body:
                - A JSON `Order` object
            - Response:
                - An array of `Fill` objects corresponding to all the matches that occurred.
    - `/:hash`
        - `GET`: Get an order by EIP712 hash
        - `DELETE`: Cancel an order by EIP712 hash
- `/book`
    - `/`
        - `GET`: Get a snapshot of the order book using [level 2 information](https://www.thebalance.com/order-book-level-2-market-data-and-depth-of-market-1031118). This `L2OrderBook` object should include the best 50 bids and best 50 asks.

## Matching Engine

### Accounts

User balances are an important part of any exchange as they define the set of valid transactions that a user can make. In the real world, exchanges allow users to deposit funds either using traditional payment rails or using blockchain technology. 
In this project, the `/accounts` route of our REST API will provide us with a convenient interface for setting up initial user balances. our matching engine only needs to support user balances of `DDX` and `USD` since the matching engine will only support orders for the `DDX/USD` pair.

This project provides basic account managements and a user cannot place order larger than their available balance. The order amount is pending before it was filled or canceled.

### Orders

[Central limit order books](https://en.wikipedia.org/wiki/Central_limit_order_book) are the most popular mechanism that exchanges use to facilitate trading. A "limit order" is a commitment to buy or sell one asset for another asset at a specified price.

Each market is denominated in one of the two assets that are being traded. A "bid" is a limit order that specifies the worst price at which a user would _buy_ the base asset using the quote asset. An "ask" is a limit order that specifies the worst price at which a user would _sell_ the base asset in exchange for the quote asset. Cryptocurrency tradable pairs are of the format <baseCurrency>/<quoteCurrency>. 
For the purposes of this project, we will be working with the DDX/USD pair. Therefore, a "bid" order suggests we are using our USD to buy DDX; an "ask" order implies we are trying to sell DDX for USD.

### Matching Rules

Whenever an order is placed on the order book with a price wider than or equal to one or more orders on the opposing side, a matching engine will match the order with some combination of the opposing orders and settle the trade by transfering assets between the buyer and seller. To better understand what wider than or equal to pricing means, let's consider a "bid" limit order. It will match with a set of "ask" orders if and only if its price is higher than or equal to the asks in consideration. Alternatively, an "ask" limit order will match with a set of "bid" orders if and only if its price is lower than or equal to the bids in consideration. Orders that are matched should have their `amount` reduced by the fill amount, and any order with an `amount` of zero should be removed from the book. The price of a match is the price of the order that was previously in the book, which means that the sender of the new order gets a better price than they asked for.

our matching engine will need to break ties between all the orders that match an incoming order. The first metric that is used to make the decision is the price. An order with a better price than another order will always be matched first. In the event that multiple orders have the best price, the oldest order is taken first.

Another feature that our matching engine should implement is "self-match prevention." Since real-world exchanges charge fees, we want to ensure that a user's order isn't matched against an order that they previously posted. Whenever the matching algorithm determines that the next matchable order's trader is the same as the submitted order's trader, it will completely cancel and discard the remainder of the submitted order regardless of how much is left to still match.


## Testing

### Testing REST API
Use Curl with below commands to test the API if running on a localhost.

To create a new account:
```shell
curl -XPOST 127.0.0.1:4321/accounts -H "Content-Type: application/json" -d '{"ddxBalance":"0.0", "usdBalance":"10.0", "traderAddress": "0xb794f5ea0ba39494ce839613fffba74279579268"}'
```

To get data from existing account:
```shell
curl -XGET -i 127.0.0.1:4321/accounts/0xb794f5ea0ba39494ce839613fffba74279579268
```

To delete an existing account:
```shell
curl -XDELETE -i 127.0.0.1:4321/accounts/0xb794f5ea0ba39494ce839613fffba74279579268
```