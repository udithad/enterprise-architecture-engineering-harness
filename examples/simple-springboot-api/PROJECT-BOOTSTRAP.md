# Simple Spring Boot API — Project Bootstrap

## 1. Project Overview

Build a simple REST API using Java 21 and Spring Boot 3.x.

The application is intended for local development and demonstration of the EAEH engineering lifecycle.

## 2. Technology Stack

- Java 21
- Spring Boot 3.x
- Maven
- REST APIs
- JUnit 5
- Spring Boot Test

## 3. Functional Scope

### 3.1 Welcome API

Endpoint: `GET /api/hello`

Return a welcome message.

### 3.2 Application Information API

Endpoint: `GET /api/info`

Return:
- Application name
- Application version
- Application description

### 3.3 Products API

Endpoint: `GET /api/products`

Return three predefined products.

Each product contains:
- ID
- Name
- Description
- Price

## 4. Architecture Constraints

- Use a simple layered architecture.
- Separate controllers, services and models.
- Store product information in memory.
- Do not use a database.
- Do not implement authentication or authorization.
- Do not include Spring Security.
- Avoid unnecessary design complexity.

## 5. Quality Requirements

- Unit tests for service logic.
- Integration tests for REST endpoints.
- Appropriate HTTP status codes.
- Consistent JSON responses.
- Maintainable and readable code.

## 6. Out of Scope

- Database and persistent storage.
- Authentication and authorization.
- External integrations.
- Distributed architecture.
- Production deployment.
- Cloud infrastructure.