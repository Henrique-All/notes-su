// src/backend/index.js
import express from 'express'
import cors from 'cors'
import bodyParser from 'body-parser'
import usersControllers from './controlles/userControllers/usersControllers.js'

import { db } from './db/index.js'
const app = express()

// Habilitando CORS
app.use(cors())

// Configurando bodyParser
app.use(bodyParser.json())
app.use(
  bodyParser.urlencoded({
    extended: false,
  }),
)

usersControllers(app)

// Conectar o servidor
app.listen(5001, () => {
  console.log('Servidor iniciado na porta 5001')
  if (db) {
    console.log('Banco de dados está online')
  }
})
