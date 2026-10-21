FROM node:22-bookworm-slim
WORKDIR /app
COPY src/auth/package*.json ./
RUN npm ci --ignore-scripts
COPY src/auth/tsconfig.json ./
COPY src/auth/*.ts ./
COPY src/auth/api ./api
COPY src/auth/domain ./domain
COPY src/auth/persistence ./persistence
COPY src/auth/migrations ./migrations
COPY src/auth/scripts ./scripts
RUN npm run build
USER node
EXPOSE 3001
CMD ["npm", "start"]
