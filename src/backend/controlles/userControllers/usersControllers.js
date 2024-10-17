/* eslint-disable @typescript-eslint/no-unused-vars */
import express from 'express'
import {
  addDoc,
  collection,
  getFirestore,
  doc,
  getDoc,
  deleteDoc,
  getDocs,
} from 'firebase/firestore'
import { db } from '../../db/index.js'

const router = express.Router()

const app = getFirestore(db)

// Listar um users
router.get('/getUsers/:id', async (req, res) => {
  try {
    const id = req.params.id

    const userRef = doc(app, 'users', id)

    const docSnap = await getDoc(userRef)

    // checando se existe o usuário no banco e retornando uma resposta com as infor do usuário
    if (docSnap.exists()) {
      const user = docSnap.data()

      console.log(user)

      return res.status(200).send(user)
    } else {
      console.log('não foi achado o usuário')

      return res.status(500).send('Usuário não encontrado')
    }
  } catch (error) {
    return res.status(400).send({ error: 'servidor nao esta respondendo' })
  }
})

// Listar todos os usuarios
router.get('/allUsers', async (req, res) => {
  try {
    const users = await getDocs(collection(app, 'users'))

    const allusers = []

    users.forEach((docs) => {
      console.log(docs.data())

      allusers.push(docs.data())
    })

    if (allusers.length !== 0) {
      res.status(200).send(allusers)
    } else {
      res.status(404).send('Houve um erro ao puxar todos os usuários!! ')
    }
  } catch (error) {
    return res.status(400).send({ error: 'servidor nao esta respondendo' })
  }
})

// criar user

router.post('/addUsers', async (req, res) => {
  const { name, email, password, confirmPassword, term, spam } = req.body

  try {
    await addDoc(collection(app, 'users'), {
      name,
      email,
      password,
      confirmPassword,
      term,
      spam,
    })

    console.log('usuário criado!!')

    return res.status(200).send('usuário criado com sucesso!')
  } catch (error) {
    return res.status(400).send({ error: 'servidor nao esta respondendo' })
  }
})

router.put('/upUsers/:id', (req, res) => {
  try {
    const id = req.params.id

    const newData = req.body

    return res.status(200).send({
      id: id,
      newData: newData,
    })
  } catch (error) {
    return res.status(400).send({ error: 'servidor nao esta respondendo' })
  }
})

// deletar usuarios

router.delete('/delUsers/:id', async (req, res) => {
  try {
    const id = req.params.id

    const userRef = doc(app, 'users', id)

    const docSnap = await getDoc(userRef)

    if (docSnap.exists()) {
      // deletando usuário

      console.log(docSnap.data())

      deleteDoc(doc(app, 'users', id))

      res.status(200).send('usuário deletado!')
    } else {
      res.send('usuário não deletado!')
    }
  } catch (error) {
    return res.status(400).send({ error: 'servidor nao esta respondendo' })
  }
})

export default (app) => {
  app.use('/users', router)
}
